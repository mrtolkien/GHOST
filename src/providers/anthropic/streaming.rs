/// Anthropic Messages API SSE stream parsing.
///
/// Accumulates streamed SSE events into a complete `ChatResponse`. Handles
/// text, tool_use, thinking, and redacted_thinking content block types.
use serde_json::Value;

use super::tool_names::from_claude_code_name;
use crate::providers::ProviderError;
use crate::providers::types::{ChatResponse, ContentBlock, StopReason, Usage};

/// In-progress state for a single content block being streamed.
#[derive(Debug, Default)]
struct BlockState {
    block_type: String,
    text: String,
    json_buf: String,
    thinking: String,
    signature: String,
    tool_id: String,
    tool_name: String,
    /// Full JSON for redacted_thinking blocks (captured at start).
    redacted_json: Option<Value>,
}

/// Parse a complete SSE response body into a `ChatResponse`.
///
/// `ghost_tool_names` is used to reverse-translate Claude Code canonical
/// tool names back to Ghost tool names.
pub(crate) fn parse_sse_response(
    raw: &str,
    fallback_model: &str,
    ghost_tool_names: &[&str],
) -> Result<ChatResponse, ProviderError> {
    let mut response_id: Option<String> = None;
    let mut model = fallback_model.to_string();
    let mut usage = Usage::default();
    let mut stop_reason = StopReason::EndTurn;
    let mut blocks: Vec<BlockState> = Vec::new();
    let mut content: Vec<ContentBlock> = Vec::new();
    let mut sensitive = false;

    for chunk in raw.split("\n\n") {
        let chunk = chunk.trim();
        if chunk.is_empty() {
            continue;
        }

        let (event_type, data) = parse_sse_chunk(chunk);
        let Some(event_type) = event_type else {
            continue;
        };
        let Some(data_str) = data else {
            continue;
        };

        match event_type {
            "message_start" => {
                let v: Value = parse_json(&data_str)?;
                let msg = &v["message"];
                if let Some(id) = msg["id"].as_str() {
                    response_id = Some(id.to_string());
                }
                if let Some(m) = msg["model"].as_str() {
                    model = m.to_string();
                }
                apply_message_start_usage(&mut usage, &msg["usage"]);
            }
            "content_block_start" => {
                let v: Value = parse_json(&data_str)?;
                let idx = v["index"].as_u64().unwrap_or(0) as usize;
                let cb = &v["content_block"];
                let block_type = cb["type"].as_str().unwrap_or("text").to_string();

                let mut state = BlockState {
                    block_type: block_type.clone(),
                    ..Default::default()
                };

                match block_type.as_str() {
                    "tool_use" => {
                        state.tool_id = cb["id"].as_str().unwrap_or("").to_string();
                        state.tool_name = cb["name"].as_str().unwrap_or("").to_string();
                    }
                    "redacted_thinking" => {
                        state.redacted_json = Some(cb.clone());
                    }
                    _ => {}
                }

                // Ensure the vec is large enough for this index.
                while blocks.len() <= idx {
                    blocks.push(BlockState::default());
                }
                blocks[idx] = state;
            }
            "content_block_delta" => {
                let v: Value = parse_json(&data_str)?;
                let idx = v["index"].as_u64().unwrap_or(0) as usize;
                if idx >= blocks.len() {
                    continue;
                }
                let delta = &v["delta"];
                let delta_type = delta["type"].as_str().unwrap_or("");

                match delta_type {
                    "text_delta" => {
                        if let Some(t) = delta["text"].as_str() {
                            blocks[idx].text.push_str(t);
                        }
                    }
                    "input_json_delta" => {
                        if let Some(j) = delta["partial_json"].as_str() {
                            blocks[idx].json_buf.push_str(j);
                        }
                    }
                    "thinking_delta" => {
                        if let Some(t) = delta["thinking"].as_str() {
                            blocks[idx].thinking.push_str(t);
                        }
                    }
                    "signature_delta" => {
                        if let Some(s) = delta["signature"].as_str() {
                            blocks[idx].signature = s.to_string();
                        }
                    }
                    _ => {}
                }
            }
            "content_block_stop" => {
                let v: Value = parse_json(&data_str)?;
                let idx = v["index"].as_u64().unwrap_or(0) as usize;
                if idx >= blocks.len() {
                    continue;
                }
                let state = &blocks[idx];
                let block = finalize_block(state, ghost_tool_names);
                content.push(block);
            }
            "message_delta" => {
                let v: Value = parse_json(&data_str)?;
                if let Some(sr) = v["delta"]["stop_reason"].as_str() {
                    match sr {
                        "sensitive" => sensitive = true,
                        "end_turn" => stop_reason = StopReason::EndTurn,
                        "tool_use" => stop_reason = StopReason::ToolUse,
                        "max_tokens" => {
                            stop_reason = StopReason::MaxTokens;
                        }
                        _ => stop_reason = StopReason::EndTurn,
                    }
                }
                if let Some(out) = v["usage"]["output_tokens"].as_u64() {
                    usage.output_tokens = out as u32;
                }
            }
            "error" => {
                let v: Value = parse_json(&data_str)?;
                let msg = v["error"]["message"]
                    .as_str()
                    .or_else(|| v["message"].as_str())
                    .unwrap_or("unknown error");
                if ProviderError::is_context_overflow_message(msg) {
                    return Err(ProviderError::ContextOverflow(msg.to_string()));
                }
                if ProviderError::is_thinking_block_incompatible(msg) {
                    return Err(ProviderError::IncompatibleHistory(msg.to_string()));
                }
                return Err(ProviderError::InvalidResponse(msg.to_string()));
            }
            // ping, message_stop, unknown — ignore
            _ => {}
        }
    }

    if sensitive {
        return Err(ProviderError::InvalidResponse(
            "content flagged as sensitive".to_string(),
        ));
    }

    if content.is_empty() {
        return Err(ProviderError::EmptyResponse {
            detail: "no content blocks in SSE stream".to_string(),
        });
    }

    Ok(ChatResponse {
        content,
        usage,
        stop_reason,
        model,
        response_id,
        turn_state: None,
    })
}

/// Extract event type and data payload from an SSE chunk.
/// Returns `(Some(event_type), Some(concatenated_data_lines))`.
fn parse_sse_chunk(chunk: &str) -> (Option<&str>, Option<String>) {
    let mut event_type = None;
    let mut data_parts: Vec<&str> = Vec::new();

    for line in chunk.lines() {
        if let Some(rest) = line.strip_prefix("event:") {
            event_type = Some(rest.trim());
        } else if let Some(rest) = line.strip_prefix("data:") {
            data_parts.push(rest.trim());
        }
    }

    let data = if data_parts.is_empty() {
        None
    } else {
        Some(data_parts.join(""))
    };

    (event_type, data)
}

fn parse_json(s: &str) -> Result<Value, ProviderError> {
    serde_json::from_str(s)
        .map_err(|e| ProviderError::InvalidResponse(format!("invalid JSON in SSE data: {e}")))
}

fn apply_message_start_usage(usage: &mut Usage, v: &Value) {
    if let Some(n) = v["input_tokens"].as_u64() {
        usage.input_tokens = n as u32;
    }
    if let Some(n) = v["cache_read_input_tokens"].as_u64() {
        usage.cache_read_tokens = Some(n as u32);
    }
    if let Some(n) = v["cache_creation_input_tokens"].as_u64() {
        usage.cache_creation_tokens = Some(n as u32);
    }
}

fn finalize_block(state: &BlockState, ghost_tool_names: &[&str]) -> ContentBlock {
    match state.block_type.as_str() {
        "tool_use" => {
            let input: Value = if state.json_buf.is_empty() {
                serde_json::json!({})
            } else {
                serde_json::from_str(&state.json_buf).unwrap_or(serde_json::json!({}))
            };
            let name = from_claude_code_name(&state.tool_name, ghost_tool_names);
            ContentBlock::ToolUse {
                id: state.tool_id.clone(),
                name,
                input,
            }
        }
        "thinking" => ContentBlock::Thinking {
            text: Some(state.thinking.clone()),
            signature: Some(state.signature.clone()),
            opaque_data: None,
        },
        "redacted_thinking" => ContentBlock::Thinking {
            text: None,
            signature: None,
            opaque_data: state
                .redacted_json
                .as_ref()
                .and_then(|v| v.get("data"))
                .and_then(Value::as_str)
                .map(String::from),
        },
        // text and anything else
        _ => ContentBlock::Text {
            text: state.text.clone(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_sse(events: &[(&str, serde_json::Value)]) -> String {
        events
            .iter()
            .map(|(event, data)| format!("event: {event}\ndata: {}\n\n", data))
            .collect()
    }

    #[test]
    fn parse_simple_text_response() {
        let sse = make_sse(&[
            (
                "message_start",
                serde_json::json!({
                    "type": "message_start",
                    "message": {
                        "id": "msg_1", "model": "claude-sonnet-4-6-20250514",
                        "usage": {"input_tokens": 10, "output_tokens": 0}
                    }
                }),
            ),
            (
                "content_block_start",
                serde_json::json!({
                    "type": "content_block_start", "index": 0,
                    "content_block": {"type": "text", "text": ""}
                }),
            ),
            (
                "content_block_delta",
                serde_json::json!({
                    "type": "content_block_delta", "index": 0,
                    "delta": {"type": "text_delta", "text": "Hello"}
                }),
            ),
            (
                "content_block_delta",
                serde_json::json!({
                    "type": "content_block_delta", "index": 0,
                    "delta": {"type": "text_delta", "text": " world"}
                }),
            ),
            (
                "content_block_stop",
                serde_json::json!({
                    "type": "content_block_stop", "index": 0
                }),
            ),
            (
                "message_delta",
                serde_json::json!({
                    "type": "message_delta",
                    "delta": {"stop_reason": "end_turn"},
                    "usage": {"output_tokens": 5}
                }),
            ),
            ("message_stop", serde_json::json!({"type": "message_stop"})),
        ]);
        let resp = parse_sse_response(&sse, "fallback", &[]).unwrap();
        assert_eq!(resp.content.len(), 1);
        match &resp.content[0] {
            ContentBlock::Text { text } => {
                assert_eq!(text, "Hello world")
            }
            other => panic!("expected Text, got {other:?}"),
        }
        assert_eq!(resp.stop_reason, StopReason::EndTurn);
        assert_eq!(resp.usage.input_tokens, 10);
        assert_eq!(resp.usage.output_tokens, 5);
    }

    #[test]
    fn parse_tool_use_response() {
        let sse = make_sse(&[
            (
                "message_start",
                serde_json::json!({
                    "type": "message_start",
                    "message": {"id": "msg_1", "model": "claude-sonnet-4-6-20250514",
                        "usage": {"input_tokens": 10, "output_tokens": 0}}
                }),
            ),
            (
                "content_block_start",
                serde_json::json!({
                    "type": "content_block_start", "index": 0,
                    "content_block": {"type": "tool_use", "id": "toolu_1", "name": "Read", "input": {}}
                }),
            ),
            (
                "content_block_delta",
                serde_json::json!({
                    "type": "content_block_delta", "index": 0,
                    "delta": {"type": "input_json_delta", "partial_json": "{\"path\":"}
                }),
            ),
            (
                "content_block_delta",
                serde_json::json!({
                    "type": "content_block_delta", "index": 0,
                    "delta": {"type": "input_json_delta", "partial_json": "\"foo.rs\"}"}
                }),
            ),
            (
                "content_block_stop",
                serde_json::json!({
                    "type": "content_block_stop", "index": 0
                }),
            ),
            (
                "message_delta",
                serde_json::json!({
                    "type": "message_delta",
                    "delta": {"stop_reason": "tool_use"},
                    "usage": {"output_tokens": 20}
                }),
            ),
            ("message_stop", serde_json::json!({"type": "message_stop"})),
        ]);
        let resp = parse_sse_response(&sse, "fallback", &["read"]).unwrap();
        assert_eq!(resp.stop_reason, StopReason::ToolUse);
        match &resp.content[0] {
            ContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "toolu_1");
                assert_eq!(name, "read"); // reverse-translated from "Read"
                assert_eq!(input["path"], "foo.rs");
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    #[test]
    fn parse_thinking_block() {
        let sse = make_sse(&[
            (
                "message_start",
                serde_json::json!({
                    "type": "message_start",
                    "message": {"id": "msg_1", "model": "claude-opus-4-6-20250514",
                        "usage": {"input_tokens": 10, "output_tokens": 0}}
                }),
            ),
            (
                "content_block_start",
                serde_json::json!({
                    "type": "content_block_start", "index": 0,
                    "content_block": {"type": "thinking", "thinking": "", "signature": ""}
                }),
            ),
            (
                "content_block_delta",
                serde_json::json!({
                    "type": "content_block_delta", "index": 0,
                    "delta": {"type": "thinking_delta", "thinking": "Let me think..."}
                }),
            ),
            (
                "content_block_delta",
                serde_json::json!({
                    "type": "content_block_delta", "index": 0,
                    "delta": {"type": "signature_delta", "signature": "sig123"}
                }),
            ),
            (
                "content_block_stop",
                serde_json::json!({
                    "type": "content_block_stop", "index": 0
                }),
            ),
            (
                "content_block_start",
                serde_json::json!({
                    "type": "content_block_start", "index": 1,
                    "content_block": {"type": "text", "text": ""}
                }),
            ),
            (
                "content_block_delta",
                serde_json::json!({
                    "type": "content_block_delta", "index": 1,
                    "delta": {"type": "text_delta", "text": "Here's my answer."}
                }),
            ),
            (
                "content_block_stop",
                serde_json::json!({
                    "type": "content_block_stop", "index": 1
                }),
            ),
            (
                "message_delta",
                serde_json::json!({
                    "type": "message_delta",
                    "delta": {"stop_reason": "end_turn"},
                    "usage": {"output_tokens": 30}
                }),
            ),
            ("message_stop", serde_json::json!({"type": "message_stop"})),
        ]);
        let resp = parse_sse_response(&sse, "fallback", &[]).unwrap();
        assert_eq!(resp.content.len(), 2);
        match &resp.content[0] {
            ContentBlock::Thinking {
                text,
                signature,
                opaque_data,
            } => {
                assert_eq!(text, &Some("Let me think...".to_string()));
                assert_eq!(signature, &Some("sig123".to_string()));
                assert_eq!(opaque_data, &None);
            }
            other => panic!("expected Thinking, got {other:?}"),
        }
        assert!(matches!(
            &resp.content[1],
            ContentBlock::Text { text } if text == "Here's my answer."
        ));
    }

    #[test]
    fn parse_redacted_thinking_block() {
        let sse = make_sse(&[
            (
                "message_start",
                serde_json::json!({
                    "type": "message_start",
                    "message": {"id": "msg_1", "model": "claude-opus-4-6-20250514",
                        "usage": {"input_tokens": 10, "output_tokens": 0}}
                }),
            ),
            (
                "content_block_start",
                serde_json::json!({
                    "type": "content_block_start", "index": 0,
                    "content_block": {"type": "redacted_thinking", "data": "encrypted_payload"}
                }),
            ),
            (
                "content_block_stop",
                serde_json::json!({
                    "type": "content_block_stop", "index": 0
                }),
            ),
            (
                "content_block_start",
                serde_json::json!({
                    "type": "content_block_start", "index": 1,
                    "content_block": {"type": "text", "text": ""}
                }),
            ),
            (
                "content_block_delta",
                serde_json::json!({
                    "type": "content_block_delta", "index": 1,
                    "delta": {"type": "text_delta", "text": "Answer."}
                }),
            ),
            (
                "content_block_stop",
                serde_json::json!({
                    "type": "content_block_stop", "index": 1
                }),
            ),
            (
                "message_delta",
                serde_json::json!({
                    "type": "message_delta",
                    "delta": {"stop_reason": "end_turn"},
                    "usage": {"output_tokens": 10}
                }),
            ),
            ("message_stop", serde_json::json!({"type": "message_stop"})),
        ]);
        let resp = parse_sse_response(&sse, "fallback", &[]).unwrap();
        assert_eq!(resp.content.len(), 2);
        match &resp.content[0] {
            ContentBlock::Thinking {
                text,
                signature,
                opaque_data,
            } => {
                assert_eq!(text, &None);
                assert_eq!(signature, &None);
                assert_eq!(opaque_data, &Some("encrypted_payload".to_string()));
            }
            other => panic!("expected Thinking, got {other:?}"),
        }
    }

    #[test]
    fn sensitive_stop_reason_is_error() {
        let sse = make_sse(&[
            (
                "message_start",
                serde_json::json!({
                    "type": "message_start",
                    "message": {"id": "msg_1", "model": "claude-sonnet-4-6-20250514",
                        "usage": {"input_tokens": 10, "output_tokens": 0}}
                }),
            ),
            (
                "message_delta",
                serde_json::json!({
                    "type": "message_delta",
                    "delta": {"stop_reason": "sensitive"},
                    "usage": {"output_tokens": 0}
                }),
            ),
            ("message_stop", serde_json::json!({"type": "message_stop"})),
        ]);
        let result = parse_sse_response(&sse, "fallback", &[]);
        assert!(result.is_err());
    }

    #[test]
    fn usage_from_message_start_preserved() {
        let sse = make_sse(&[
            (
                "message_start",
                serde_json::json!({
                    "type": "message_start",
                    "message": {"id": "msg_1", "model": "claude-sonnet-4-6-20250514",
                        "usage": {"input_tokens": 42, "output_tokens": 0,
                                  "cache_read_input_tokens": 100,
                                  "cache_creation_input_tokens": 50}}
                }),
            ),
            (
                "content_block_start",
                serde_json::json!({
                    "type": "content_block_start", "index": 0,
                    "content_block": {"type": "text", "text": ""}
                }),
            ),
            (
                "content_block_delta",
                serde_json::json!({
                    "type": "content_block_delta", "index": 0,
                    "delta": {"type": "text_delta", "text": "hi"}
                }),
            ),
            (
                "content_block_stop",
                serde_json::json!({
                    "type": "content_block_stop", "index": 0
                }),
            ),
            (
                "message_delta",
                serde_json::json!({
                    "type": "message_delta",
                    "delta": {"stop_reason": "end_turn"},
                    "usage": {"output_tokens": 1}
                }),
            ),
            ("message_stop", serde_json::json!({"type": "message_stop"})),
        ]);
        let resp = parse_sse_response(&sse, "fallback", &[]).unwrap();
        assert_eq!(resp.usage.input_tokens, 42);
        assert_eq!(resp.usage.output_tokens, 1);
        assert_eq!(resp.usage.cache_read_tokens, Some(100));
        assert_eq!(resp.usage.cache_creation_tokens, Some(50));
    }
}
