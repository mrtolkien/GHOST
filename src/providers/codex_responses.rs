use serde::Serialize;
use serde_json::{Value, json};

use crate::providers::types::{
    ChatRequest, ChatResponse, ContentBlock, ProviderError, StopReason, Usage,
};

#[derive(Debug, Serialize)]
pub(super) struct CodexResponsesRequest {
    pub model: String,
    pub store: bool,
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    pub input: Vec<CodexInputItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<CodexToolDefinition>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<CodexReasoning>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<CodexText>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_cache_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_cache_retention: Option<String>,
}

/// Codex Responses API reasoning configuration.
#[derive(Debug, Serialize)]
pub(super) struct CodexReasoning {
    pub effort: String,
}

/// Codex Responses API text output configuration.
#[derive(Debug, Serialize)]
pub(super) struct CodexText {
    pub verbosity: String,
}

/// Responses API uses a flat tool format (no nested `function` object).
/// `strict` is omitted (defaults to false) because our tool schemas have
/// optional parameters and OpenAI strict mode requires all properties in
/// `required`.
#[derive(Debug, Serialize)]
pub(super) struct CodexToolDefinition {
    r#type: String,
    name: String,
    description: String,
    parameters: Value,
}

/// Items in the Responses API `input` array. The array is heterogeneous:
/// messages, function calls, and function call outputs are distinct item types.
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum CodexInputItem {
    FunctionCall {
        call_id: String,
        name: String,
        arguments: String,
    },
    FunctionCallOutput {
        call_id: String,
        output: String,
    },
    /// Opaque item echoed back verbatim (e.g. reasoning items).
    /// Serialized as raw JSON — the `type` tag comes from the value itself.
    #[serde(untagged)]
    Raw(Value),
}

/// Convert a generic `ChatRequest` into the Codex Responses API wire format.
///
/// Maps our internal message/tool representations to the flat `input` array
/// that the Responses API expects (messages, function_calls, function_call_outputs).
pub(super) fn build_codex_request_body(
    request: &ChatRequest,
) -> Result<CodexResponsesRequest, ProviderError> {
    let mut input = Vec::new();
    for message in &request.messages {
        let (role, part_type) = match message.role {
            crate::providers::Role::User => ("user", "input_text"),
            crate::providers::Role::Assistant => ("assistant", "output_text"),
            crate::providers::Role::System => ("developer", "input_text"),
        };

        // Collect raw output items (e.g. reasoning) — they must appear as
        // top-level input items *before* the message they belong to.
        let mut raw_items = Vec::new();
        for block in &message.content {
            if let ContentBlock::RawOutput { value, .. } = block {
                raw_items.push(value.clone());
            }
        }
        // Push raw items before the message (API expects reasoning → message).
        for raw in raw_items {
            input.push(CodexInputItem::Raw(raw));
        }

        // Collect text parts and image parts into a message item.
        let mut content_parts: Vec<Value> = Vec::new();
        for block in &message.content {
            match block {
                ContentBlock::Text { text } if !text.trim().is_empty() => {
                    content_parts.push(json!({
                        "type": part_type,
                        "text": text,
                    }));
                }
                ContentBlock::Image { path, .. } => {
                    match crate::images::load_image_base64(std::path::Path::new(path)) {
                        Ok((b64, mime)) => {
                            content_parts.push(json!({
                                "type": "input_image",
                                "image_url": format!("data:{mime};base64,{b64}"),
                            }));
                        }
                        Err(e) => {
                            content_parts.push(json!({
                                "type": part_type,
                                "text": format!("[Image unavailable: {e}]"),
                            }));
                        }
                    }
                }
                _ => {}
            }
        }
        if !content_parts.is_empty() {
            // Use Raw variant to emit heterogeneous content parts
            input.push(CodexInputItem::Raw(json!({
                "type": "message",
                "role": role,
                "content": content_parts,
            })));
        }

        // Tool use and tool result blocks become separate input items.
        for block in &message.content {
            match block {
                ContentBlock::ToolUse {
                    id,
                    name,
                    input: tool_input,
                } => {
                    let arguments =
                        serde_json::to_string(tool_input).unwrap_or_else(|_| "{}".to_string());
                    input.push(CodexInputItem::FunctionCall {
                        call_id: id.clone(),
                        name: name.clone(),
                        arguments,
                    });
                }
                ContentBlock::ToolResult {
                    tool_use_id,
                    content,
                    ..
                } => {
                    input.push(CodexInputItem::FunctionCallOutput {
                        call_id: tool_use_id.clone(),
                        output: content.clone(),
                    });
                }
                ContentBlock::Text { .. }
                | ContentBlock::Image { .. }
                | ContentBlock::RawOutput { .. } => {}
            }
        }
    }

    if input.is_empty() {
        return Err(ProviderError::InvalidResponse(
            "request must include at least one input item".to_string(),
        ));
    }

    let tools = request.tools.as_ref().map(|tools| {
        tools
            .iter()
            .map(|tool| CodexToolDefinition {
                r#type: "function".to_string(),
                name: tool.name.clone(),
                description: tool.description.clone(),
                parameters: tool.input_schema.clone(),
            })
            .collect()
    });
    let tool_choice = tools.as_ref().map(|_| "auto".to_string());

    let cache_key = if request.cache_key.is_empty() {
        None
    } else {
        Some(request.cache_key.clone())
    };

    Ok(CodexResponsesRequest {
        model: request.model.clone(),
        store: false,
        stream: true,
        instructions: request.system.clone(),
        input,
        tools,
        tool_choice,
        include: Some(vec!["reasoning.encrypted_content".to_string()]),
        reasoning: request.reasoning_effort.map(|e| CodexReasoning {
            effort: e.as_str().to_string(),
        }),
        text: Some(CodexText {
            verbosity: "low".to_string(),
        }),
        prompt_cache_key: cache_key,
        prompt_cache_retention: None,
    })
}

/// Parse a Codex Responses API response body.
///
/// With `stream: false` (our default), the body is a single JSON object.
/// With `stream: true` (or if the server ignores the flag), the body is SSE.
/// This function tries JSON first, then falls back to SSE parsing.
pub(super) fn parse_codex_response(
    raw: &str,
    fallback_model: &str,
) -> Result<ChatResponse, ProviderError> {
    let trimmed = raw.trim();

    // Non-streaming: body is a single JSON response object.
    if trimmed.starts_with('{') {
        return parse_codex_json_response(trimmed, fallback_model);
    }

    // Streaming fallback: body contains SSE events.
    parse_codex_sse_response(trimmed, fallback_model)
}

/// Parse a non-streaming JSON response (the response object directly).
fn parse_codex_json_response(
    raw: &str,
    fallback_model: &str,
) -> Result<ChatResponse, ProviderError> {
    let value: Value = serde_json::from_str(raw).map_err(|e| {
        ProviderError::InvalidResponse(format!("failed to parse JSON response: {e}"))
    })?;

    // Check for failed status.
    if value.get("status").and_then(Value::as_str) == Some("failed") {
        let error_msg = value
            .pointer("/error/message")
            .and_then(Value::as_str)
            .unwrap_or("unknown error");
        logfire::error!(
            "codex response failed",
            error = error_msg.to_string(),
            status = "failed",
        );
        return Err(ProviderError::InvalidResponse(format!(
            "codex response failed: {error_msg}"
        )));
    }

    parse_codex_response_value(&value, fallback_model)
}

/// Parse SSE streaming response (fallback if server sends SSE despite stream=false).
fn parse_codex_sse_response(
    raw: &str,
    fallback_model: &str,
) -> Result<ChatResponse, ProviderError> {
    let mut completed_response: Option<Value> = None;
    let mut output_text = String::new();
    // Fallback: collect completed output items in case response.completed is missing.
    let mut done_items: Vec<Value> = Vec::new();
    let mut event_types_seen: Vec<String> = Vec::new();

    for chunk in raw.split("\n\n") {
        for line in chunk.lines() {
            let line = line.trim();
            if !line.starts_with("data:") {
                continue;
            }
            let data = line.trim_start_matches("data:").trim();
            if data.is_empty() || data == "[DONE]" {
                continue;
            }
            let event_value: Value = match serde_json::from_str(data) {
                Ok(value) => value,
                Err(_) => continue,
            };
            let event_type = event_value
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("");
            event_types_seen.push(event_type.to_string());
            match event_type {
                "response.output_text.delta" => {
                    if let Some(delta) = event_value.get("delta").and_then(Value::as_str) {
                        output_text.push_str(delta);
                    }
                }
                "response.completed" | "response.incomplete" => {
                    if let Some(response) = event_value.get("response") {
                        completed_response = Some(response.clone());
                    }
                }
                // Collect completed output items as fallback data.
                "response.output_item.done" => {
                    if let Some(item) = event_value.get("item") {
                        done_items.push(item.clone());
                    }
                }
                "response.failed" => {
                    let error_msg = event_value
                        .pointer("/response/error/message")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown error");
                    logfire::error!("codex SSE: response.failed", error = error_msg.to_string(),);
                    return Err(ProviderError::InvalidResponse(format!(
                        "codex response failed: {error_msg}"
                    )));
                }
                _ => {}
            }
        }
    }

    // Primary path: use the authoritative response.completed/incomplete event.
    if let Some(value) = completed_response {
        return parse_codex_response_value(&value, fallback_model);
    }

    // Fallback: reconstruct from individual done items if terminal event was missing.
    if !done_items.is_empty() {
        logfire::warn!(
            "codex SSE: no terminal event, reconstructing from output_item.done events",
            item_count = done_items.len() as u64,
            events = event_types_seen.join(", "),
        );
        let synthetic = serde_json::json!({
            "model": fallback_model,
            "status": "completed",
            "output": done_items,
        });
        return parse_codex_response_value(&synthetic, fallback_model);
    }

    // Fallback: use accumulated streaming text.
    if !output_text.trim().is_empty() {
        return Ok(ChatResponse {
            content: vec![ContentBlock::Text { text: output_text }],
            usage: Usage::default(),
            stop_reason: StopReason::EndTurn,
            model: fallback_model.to_string(),
            response_id: None,
            turn_state: None,
        });
    }

    let raw_head: String = raw.chars().take(500).collect();
    let events = event_types_seen.join(", ");
    let detail = format!(
        "codex SSE: no terminal event, no output items, no text (events: {events}, raw_len: {})",
        raw.len()
    );
    logfire::warn!(
        "codex SSE: empty response — no terminal event, no output items, no text",
        events = detail.clone(),
        raw_len = raw.len() as u64,
        raw_head = raw_head,
    );
    Err(ProviderError::EmptyResponse { detail })
}

/// Extract a human-readable summary from a reasoning output item.
///
/// Reads `item.summary[*].text`, joins with spaces, truncates to 200 chars.
/// Used for logging only — the full value is preserved in `RawOutput`.
pub(crate) fn extract_reasoning_summary(item: &Value) -> String {
    let summary = item
        .get("summary")
        .and_then(Value::as_array)
        .map(|parts| {
            parts
                .iter()
                .filter_map(|p| p.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();
    if summary.len() > 200 {
        let end = summary.floor_char_boundary(200);
        format!("{}...", &summary[..end])
    } else {
        summary
    }
}

/// Parse a Codex Responses API response object into our internal `ChatResponse`.
///
/// Walks the `output` array, converting messages to text blocks, function_calls
/// to tool use blocks, and preserving opaque items (e.g. reasoning) as `RawOutput`.
/// Also extracts usage and determines the stop reason.
pub(super) fn parse_codex_response_value(
    value: &Value,
    fallback_model: &str,
) -> Result<ChatResponse, ProviderError> {
    let model = value
        .get("model")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .unwrap_or_else(|| fallback_model.to_string());
    let stop_reason = match value.get("status").and_then(Value::as_str) {
        Some("incomplete") => StopReason::MaxTokens,
        Some("completed") => StopReason::EndTurn,
        _ => StopReason::EndTurn,
    };

    let mut content = Vec::new();
    if let Some(output) = value.get("output").and_then(Value::as_array) {
        for item in output {
            let item_type = item.get("type").and_then(Value::as_str).unwrap_or("");
            match item_type {
                "message" => {
                    let mut found_text = false;
                    if let Some(parts) = item.get("content").and_then(Value::as_array) {
                        for part in parts {
                            if let Some(text) = part
                                .get("text")
                                .and_then(Value::as_str)
                                .or_else(|| part.get("output_text").and_then(Value::as_str))
                                && !text.trim().is_empty()
                            {
                                content.push(ContentBlock::Text {
                                    text: text.to_string(),
                                });
                                found_text = true;
                            }
                        }
                    }
                    if !found_text {
                        content.push(ContentBlock::RawOutput {
                            original_type: "message".to_string(),
                            value: item.clone(),
                        });
                    }
                }
                "function_call" => {
                    if let (Some(call_id), Some(name)) = (
                        item.get("call_id").and_then(Value::as_str),
                        item.get("name").and_then(Value::as_str),
                    ) {
                        let arguments = item
                            .get("arguments")
                            .and_then(Value::as_str)
                            .unwrap_or("{}");
                        let input = serde_json::from_str(arguments)
                            .unwrap_or(Value::Object(serde_json::Map::new()));
                        content.push(ContentBlock::ToolUse {
                            id: call_id.to_string(),
                            name: name.to_string(),
                            input,
                        });
                    } else {
                        logfire::warn!(
                            "codex: malformed function_call item (missing call_id or name)",
                            item = item.to_string(),
                        );
                        content.push(ContentBlock::RawOutput {
                            original_type: "function_call".to_string(),
                            value: item.clone(),
                        });
                    }
                }
                other => {
                    let reasoning_summary = extract_reasoning_summary(item);
                    logfire::info!(
                        "codex: preserving opaque output item",
                        item_type = other.to_string(),
                        reasoning_summary = reasoning_summary,
                    );
                    content.push(ContentBlock::RawOutput {
                        original_type: other.to_string(),
                        value: item.clone(),
                    });
                }
            }
        }
    }
    if content.is_empty()
        && let Some(text) = value
            .get("output_text")
            .and_then(Value::as_str)
            .filter(|text| !text.trim().is_empty())
    {
        content.push(ContentBlock::Text {
            text: text.to_string(),
        });
    }

    if content.is_empty() {
        let status = value
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let output_len = value
            .get("output")
            .and_then(Value::as_array)
            .map_or(0, |a| a.len());
        let output_text_present = value.get("output_text").is_some();
        let raw_preview: String = value.to_string().chars().take(500).collect();
        let status_owned = status.to_string();
        logfire::warn!(
            "codex response has no content blocks",
            status = status_owned,
            model = model.clone(),
            output_items = output_len as u64,
            output_text_present = output_text_present,
            raw_preview = raw_preview,
        );
        return Err(ProviderError::EmptyResponse {
            detail: format!(
                "codex response has no content blocks (status: {status}, \
                 model: {model}, output_items: {output_len})"
            ),
        });
    }

    // Override stop_reason when the model made tool calls.
    let has_tool_calls = content
        .iter()
        .any(|block| matches!(block, ContentBlock::ToolUse { .. }));
    let stop_reason = if has_tool_calls {
        StopReason::ToolUse
    } else {
        stop_reason
    };

    let usage_value = value.get("usage");
    let input_tokens = usage_value
        .and_then(|usage| {
            usage
                .get("input_tokens")
                .or_else(|| usage.get("prompt_tokens"))
        })
        .and_then(Value::as_u64)
        .unwrap_or(0) as u32;
    let output_tokens = usage_value
        .and_then(|usage| {
            usage
                .get("output_tokens")
                .or_else(|| usage.get("completion_tokens"))
        })
        .and_then(Value::as_u64)
        .unwrap_or(0) as u32;
    let cache_read_tokens = usage_value
        .and_then(|usage| {
            usage
                .get("input_tokens_details")
                .and_then(|details| details.get("cached_tokens"))
                .or_else(|| usage.get("cache_read_input_tokens"))
        })
        .and_then(Value::as_u64)
        .map(|value| value as u32);
    let cache_creation_tokens = usage_value
        .and_then(|usage| {
            usage
                .get("input_tokens_details")
                .and_then(|details| details.get("cache_creation_tokens"))
                .or_else(|| usage.get("cache_creation_input_tokens"))
        })
        .and_then(Value::as_u64)
        .map(|value| value as u32);

    let response_id = value
        .get("id")
        .and_then(Value::as_str)
        .map(ToString::to_string);

    Ok(ChatResponse {
        content,
        usage: Usage {
            input_tokens,
            output_tokens,
            cache_read_tokens,
            cache_creation_tokens,
        },
        stop_reason,
        model,
        response_id,
        turn_state: None,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::providers::types::ReasoningEffort;
    use crate::providers::{ChatMessage, Role, ToolDefinition};

    #[test]
    fn build_request_uses_output_text_for_assistant_messages() {
        let request = ChatRequest {
            messages: vec![
                ChatMessage {
                    role: Role::User,
                    content: vec![ContentBlock::Text {
                        text: "user text".to_string(),
                    }],
                },
                ChatMessage {
                    role: Role::Assistant,
                    content: vec![ContentBlock::Text {
                        text: "assistant text".to_string(),
                    }],
                },
            ],
            model: "gpt-5.3-codex".to_string(),
            max_tokens: None,
            temperature: None,
            system: None,
            tools: None,
            reasoning_effort: None,
            cache_key: "test".to_string(),
            turn_state: None,
            debug_context: None,
        };

        let body = build_codex_request_body(&request).expect("request body");
        let json = serde_json::to_value(&body).expect("serialize");
        assert_eq!(json["input"][0]["role"], "user");
        assert_eq!(json["input"][0]["content"][0]["type"], "input_text");
        assert_eq!(json["input"][1]["role"], "assistant");
        assert_eq!(json["input"][1]["content"][0]["type"], "output_text");
    }

    #[test]
    fn build_request_includes_tools_and_tool_choice() {
        let request = ChatRequest {
            messages: vec![ChatMessage {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: "run pwd".to_string(),
                }],
            }],
            model: "gpt-5.3-codex".to_string(),
            tools: Some(vec![ToolDefinition {
                name: "shell".to_string(),
                description: "Run a shell command".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": { "command": { "type": "string" } },
                    "required": ["command"]
                }),
            }]),
            max_tokens: None,
            temperature: None,
            system: None,
            reasoning_effort: None,
            cache_key: "test".to_string(),
            turn_state: None,
            debug_context: None,
        };

        let body = build_codex_request_body(&request).expect("request body");
        let json = serde_json::to_value(&body).expect("serialize");

        assert_eq!(json["tool_choice"], "auto");
        assert_eq!(json["tools"][0]["type"], "function");
        assert_eq!(json["tools"][0]["name"], "shell");
        // Responses API flat format — no nested "function" object.
        assert!(json["tools"][0].get("function").is_none());
    }

    #[test]
    fn build_request_converts_tool_use_and_tool_result() {
        let request = ChatRequest {
            messages: vec![
                ChatMessage {
                    role: Role::User,
                    content: vec![ContentBlock::Text {
                        text: "run pwd".to_string(),
                    }],
                },
                ChatMessage {
                    role: Role::Assistant,
                    content: vec![ContentBlock::ToolUse {
                        id: "call_1".to_string(),
                        name: "shell".to_string(),
                        input: json!({"command": "pwd"}),
                    }],
                },
                ChatMessage {
                    role: Role::User,
                    content: vec![ContentBlock::ToolResult {
                        tool_use_id: "call_1".to_string(),
                        content: "/home/user".to_string(),
                        is_error: false,
                    }],
                },
            ],
            model: "gpt-5.3-codex".to_string(),
            tools: None,
            max_tokens: None,
            temperature: None,
            system: None,
            reasoning_effort: None,
            cache_key: "test".to_string(),
            turn_state: None,
            debug_context: None,
        };

        let body = build_codex_request_body(&request).expect("request body");
        let json = serde_json::to_value(&body).expect("serialize");

        // message, function_call, function_call_output
        assert_eq!(json["input"][0]["type"], "message");
        assert_eq!(json["input"][1]["type"], "function_call");
        assert_eq!(json["input"][1]["call_id"], "call_1");
        assert_eq!(json["input"][1]["name"], "shell");
        assert_eq!(json["input"][2]["type"], "function_call_output");
        assert_eq!(json["input"][2]["call_id"], "call_1");
        assert_eq!(json["input"][2]["output"], "/home/user");
    }

    #[test]
    fn parse_response_extracts_function_call() {
        let response_value = json!({
            "model": "gpt-5.3-codex",
            "status": "completed",
            "output": [
                {
                    "type": "function_call",
                    "call_id": "call_abc",
                    "name": "shell",
                    "arguments": "{\"command\":\"pwd\"}"
                }
            ],
            "usage": {
                "input_tokens": 100,
                "output_tokens": 20
            }
        });

        let parsed =
            parse_codex_response_value(&response_value, "fallback").expect("parse response");

        assert_eq!(parsed.stop_reason, StopReason::ToolUse);
        assert_eq!(parsed.content.len(), 1);
        match &parsed.content[0] {
            ContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "call_abc");
                assert_eq!(name, "shell");
                assert_eq!(input["command"], "pwd");
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    #[test]
    fn parse_response_handles_text_and_function_call() {
        let response_value = json!({
            "model": "gpt-5.3-codex",
            "status": "completed",
            "output": [
                {
                    "type": "message",
                    "content": [{"type": "output_text", "text": "Let me check."}]
                },
                {
                    "type": "function_call",
                    "call_id": "call_xyz",
                    "name": "file_read",
                    "arguments": "{\"path\":\"src/main.rs\"}"
                }
            ],
            "usage": { "input_tokens": 50, "output_tokens": 10 }
        });

        let parsed =
            parse_codex_response_value(&response_value, "fallback").expect("parse response");

        assert_eq!(parsed.stop_reason, StopReason::ToolUse);
        assert_eq!(parsed.content.len(), 2);
        assert!(
            matches!(&parsed.content[0], ContentBlock::Text { text } if text == "Let me check.")
        );
        assert!(
            matches!(&parsed.content[1], ContentBlock::ToolUse { name, .. } if name == "file_read")
        );
    }

    #[test]
    fn parse_response_preserves_reasoning_with_function_call() {
        let response_value = json!({
            "model": "gpt-5.3-codex",
            "status": "completed",
            "output": [
                {
                    "type": "reasoning",
                    "summary": [{"type": "summary_text", "text": "thinking about it"}]
                },
                {
                    "type": "function_call",
                    "call_id": "call_xyz",
                    "name": "file_read",
                    "arguments": "{\"path\":\"src/main.rs\"}"
                }
            ],
            "usage": { "input_tokens": 50, "output_tokens": 10 }
        });

        let parsed =
            parse_codex_response_value(&response_value, "fallback").expect("parse response");

        assert_eq!(parsed.stop_reason, StopReason::ToolUse);
        assert_eq!(parsed.content.len(), 2);
        assert!(matches!(
            &parsed.content[0],
            ContentBlock::RawOutput { original_type, .. } if original_type == "reasoning"
        ));
        assert!(matches!(
            &parsed.content[1],
            ContentBlock::ToolUse { name, .. } if name == "file_read"
        ));
    }

    #[test]
    fn parse_response_reasoning_only_returns_raw_output() {
        let response_value = json!({
            "model": "gpt-5.3-codex",
            "status": "completed",
            "output": [
                {
                    "type": "reasoning",
                    "summary": [{"type": "summary_text", "text": "just thinking"}]
                }
            ],
            "usage": { "input_tokens": 50, "output_tokens": 10 }
        });

        let parsed = parse_codex_response_value(&response_value, "fallback")
            .expect("reasoning-only should succeed");
        assert_eq!(parsed.content.len(), 1);
        assert!(matches!(
            &parsed.content[0],
            ContentBlock::RawOutput { original_type, .. } if original_type == "reasoning"
        ));
        assert_eq!(parsed.stop_reason, StopReason::EndTurn);
    }

    #[test]
    fn build_request_echoes_raw_output_as_top_level_item() {
        let reasoning_value = json!({
            "type": "reasoning",
            "id": "rs_abc",
            "summary": [{"type": "summary_text", "text": "thought"}],
            "encrypted_content": "enc_data_here"
        });
        let request = ChatRequest {
            messages: vec![
                ChatMessage {
                    role: Role::User,
                    content: vec![ContentBlock::Text {
                        text: "hello".to_string(),
                    }],
                },
                ChatMessage {
                    role: Role::Assistant,
                    content: vec![
                        ContentBlock::RawOutput {
                            original_type: "reasoning".to_string(),
                            value: reasoning_value.clone(),
                        },
                        ContentBlock::Text {
                            text: "I'll help.".to_string(),
                        },
                    ],
                },
            ],
            model: "gpt-5.3-codex".to_string(),
            tools: None,
            max_tokens: None,
            temperature: None,
            system: None,
            reasoning_effort: None,
            cache_key: "test".to_string(),
            turn_state: None,
            debug_context: None,
        };

        let body = build_codex_request_body(&request).expect("request body");
        let json = serde_json::to_value(&body).expect("serialize");

        // Raw reasoning item should appear as top-level input item before
        // the assistant message.
        assert_eq!(json["input"][1]["type"], "reasoning");
        assert_eq!(json["input"][1]["id"], "rs_abc");
        // The assistant text message should follow.
        assert_eq!(json["input"][2]["type"], "message");
        assert_eq!(json["input"][2]["role"], "assistant");
    }

    #[test]
    fn extract_reasoning_summary_extracts_text() {
        let item = json!({
            "type": "reasoning",
            "summary": [
                {"type": "summary_text", "text": "first thought"},
                {"type": "summary_text", "text": "second thought"}
            ]
        });
        let summary = super::extract_reasoning_summary(&item);
        assert_eq!(summary, "first thought second thought");
    }

    #[test]
    fn extract_reasoning_summary_empty_for_no_summary() {
        let item = json!({"type": "reasoning"});
        let summary = super::extract_reasoning_summary(&item);
        assert!(summary.is_empty());
    }

    #[test]
    fn build_request_includes_reasoning_encrypted_content() {
        let request = ChatRequest {
            messages: vec![ChatMessage {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: "hello".to_string(),
                }],
            }],
            model: "gpt-5.3-codex".to_string(),
            tools: None,
            max_tokens: None,
            temperature: None,
            system: None,
            reasoning_effort: None,
            cache_key: "test".to_string(),
            turn_state: None,
            debug_context: None,
        };

        let body = build_codex_request_body(&request).expect("request body");
        let json = serde_json::to_value(&body).expect("serialize");
        let include = json["include"].as_array().expect("include array");
        assert!(include.contains(&json!("reasoning.encrypted_content")));
    }

    #[test]
    fn build_request_includes_reasoning_effort_high() {
        let request = ChatRequest {
            messages: vec![ChatMessage {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: "hello".to_string(),
                }],
            }],
            model: "gpt-5.3-codex".to_string(),
            tools: None,
            max_tokens: None,
            temperature: None,
            system: None,
            reasoning_effort: Some(ReasoningEffort::High),
            cache_key: "test".to_string(),
            turn_state: None,
            debug_context: None,
        };

        let body = build_codex_request_body(&request).expect("request body");
        let json = serde_json::to_value(&body).expect("serialize");
        assert_eq!(json["reasoning"]["effort"], "high");
    }

    #[test]
    fn build_request_omits_reasoning_when_none() {
        let request = ChatRequest {
            messages: vec![ChatMessage {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: "hello".to_string(),
                }],
            }],
            model: "gpt-5.3-codex".to_string(),
            tools: None,
            max_tokens: None,
            temperature: None,
            system: None,
            reasoning_effort: None,
            cache_key: "test".to_string(),
            turn_state: None,
            debug_context: None,
        };

        let body = build_codex_request_body(&request).expect("request body");
        let json = serde_json::to_value(&body).expect("serialize");
        assert!(
            json.get("reasoning").is_none() || json.get("reasoning").unwrap().is_null(),
            "reasoning should be absent or null when effort is None"
        );
    }
}
