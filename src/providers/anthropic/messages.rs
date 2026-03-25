/// Ghost `ChatRequest` -> Anthropic Messages API request body conversion.
///
/// Handles system prompt prepend, tool name translation, cache control,
/// thinking config, surrogate sanitization, orphaned tool calls,
/// consecutive tool result batching, and cross-model thinking block handling.
use base64::Engine;
use regex::Regex;
use serde_json::{Value, json};
use std::sync::LazyLock;

use super::tool_names::{normalize_tool_call_id, to_claude_code_name};
use crate::providers::ProviderError;
use crate::providers::types::*;

const CLAUDE_CODE_PREAMBLE: &str = "You are Claude Code, Anthropic's official CLI for Claude.";

const DEFAULT_MAX_TOKENS: u32 = 8096;

static SURROGATE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\\u[dD][89abAB][0-9a-fA-F]{2}").expect("valid regex"));

/// Strip unpaired UTF-16 surrogate escape sequences from text.
pub(crate) fn sanitize_surrogates(text: &str) -> String {
    SURROGATE_RE.replace_all(text, "").to_string()
}

/// Detect models that support adaptive thinking (4.6+ family).
pub(super) fn is_adaptive_thinking_model(model: &str) -> bool {
    let lower = model.to_lowercase();
    lower.contains("opus-4-6")
        || lower.contains("opus-4.6")
        || lower.contains("sonnet-4-6")
        || lower.contains("sonnet-4.6")
}

/// Build the Anthropic Messages API JSON body from a Ghost `ChatRequest`.
pub(crate) fn build_request_body(
    request: &ChatRequest,
    ghost_tool_names: &[&str],
) -> Result<Value, ProviderError> {
    let mut body = json!({
        "model": request.model,
        "max_tokens": request.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
        "stream": true,
    });

    // --- System prompt (two separate blocks per pi-mono) ---
    let mut system_blocks = vec![json!({
        "type": "text",
        "text": CLAUDE_CODE_PREAMBLE,
        "cache_control": { "type": "ephemeral" }
    })];
    if let Some(ref system) = request.system {
        system_blocks.push(json!({
            "type": "text",
            "text": system,
            "cache_control": { "type": "ephemeral" }
        }));
    }
    body["system"] = Value::Array(system_blocks);

    // --- Tool definitions ---
    if let Some(tools) = &request.tools {
        let mut tool_defs: Vec<Value> = tools
            .iter()
            .map(|t| {
                json!({
                    "name": to_claude_code_name(&t.name),
                    "description": t.description,
                    "input_schema": t.input_schema,
                })
            })
            .collect();
        // Cache control on the last tool definition only.
        if let Some(last) = tool_defs.last_mut() {
            last["cache_control"] = json!({"type": "ephemeral"});
        }
        body["tools"] = Value::Array(tool_defs);
    }

    // --- Messages ---
    let messages = convert_messages(&request.messages, ghost_tool_names)?;
    body["messages"] = Value::Array(messages);

    // --- Thinking config ---
    let thinking_enabled = apply_thinking_config(request, &mut body);

    // --- Temperature (omit when thinking is enabled) ---
    if !thinking_enabled && let Some(temp) = request.temperature {
        body["temperature"] = json!(temp);
    }

    // --- metadata.user_id ---
    if let Some(ref dc) = request.debug_context
        && !dc.session_id.is_empty()
    {
        body["metadata"] = json!({"user_id": dc.session_id});
    }

    Ok(body)
}

// ---------------------------------------------------------------------------
// Message conversion
// ---------------------------------------------------------------------------

/// Convert Ghost messages to Anthropic format, handling batching, orphans,
/// and cache control.
fn convert_messages(
    messages: &[ChatMessage],
    ghost_tool_names: &[&str],
) -> Result<Vec<Value>, ProviderError> {
    let mut output: Vec<Value> = Vec::new();
    let len = messages.len();

    // System messages that appear between an assistant tool_use and
    // the user tool_result must be deferred — Anthropic requires the
    // tool_result to be in the *immediately next* message after
    // tool_use. Deferred blocks are prepended to the next user message.
    let mut deferred_system_blocks: Vec<Value> = Vec::new();

    let mut i = 0;
    while i < len {
        let msg = &messages[i];

        // --- System messages: defer if inside a tool-use/result pair ---
        if msg.role == Role::System {
            let blocks: Vec<Value> = msg
                .content
                .iter()
                .filter_map(|b| match b {
                    ContentBlock::Text { text } => {
                        let sanitized = sanitize_surrogates(text);
                        if sanitized.is_empty() {
                            None
                        } else {
                            Some(json!({
                                "type": "text",
                                "text": format!(
                                    "[Previous conversation summary]\n\
                                     {sanitized}"
                                ),
                            }))
                        }
                    }
                    _ => None,
                })
                .collect();

            if !blocks.is_empty() {
                // Check if the previous output message is an assistant
                // with tool_use — if so we must defer this system text
                // so the tool_result can be adjacent.
                let prev_is_tool_use = output.last().is_some_and(|m| {
                    m["role"] == "assistant"
                        && m["content"]
                            .as_array()
                            .is_some_and(|arr| arr.iter().any(|b| b["type"] == "tool_use"))
                });

                if prev_is_tool_use {
                    deferred_system_blocks.extend(blocks);
                } else {
                    output.push(json!({
                        "role": "user",
                        "content": blocks,
                    }));
                }
            }
            i += 1;
            continue;
        }

        let role_str = role_str(&msg.role);

        let content_blocks = convert_content_blocks(&msg.content, ghost_tool_names);

        if content_blocks.is_empty() {
            i += 1;
            continue;
        }

        // Prepend any deferred system blocks to this user message.
        let content_blocks = if msg.role == Role::User && !deferred_system_blocks.is_empty() {
            let mut merged = std::mem::take(&mut deferred_system_blocks);
            merged.extend(content_blocks);
            merged
        } else {
            // If we reach a non-user message and still have deferred
            // blocks, emit them as a separate user message first.
            if !deferred_system_blocks.is_empty() {
                output.push(json!({
                    "role": "user",
                    "content": std::mem::take(&mut deferred_system_blocks),
                }));
            }
            content_blocks
        };

        output.push(json!({
            "role": role_str,
            "content": content_blocks,
        }));

        // After an assistant message with tool_use blocks, check for
        // orphaned tool calls.
        if msg.role == Role::Assistant {
            let tool_use_ids: Vec<String> = msg
                .content
                .iter()
                .filter_map(|b| match b {
                    ContentBlock::ToolUse { id, .. } => Some(normalize_tool_call_id(id)),
                    _ => None,
                })
                .collect();

            if !tool_use_ids.is_empty() {
                let next_has_results = has_matching_tool_results(messages, i + 1, &tool_use_ids);
                if !next_has_results {
                    // Insert synthetic error results.
                    let synthetic: Vec<Value> = tool_use_ids
                        .iter()
                        .map(|id| {
                            json!({
                                "type": "tool_result",
                                "tool_use_id": id,
                                "content": "Tool execution was interrupted.",
                                "is_error": true,
                            })
                        })
                        .collect();
                    output.push(json!({
                        "role": "user",
                        "content": synthetic,
                    }));
                }
            }
        }

        // Consecutive tool result batching: if this is a user message
        // with only tool_results, merge subsequent user messages that
        // also contain only tool_results.
        if msg.role == Role::User && is_tool_result_only(&msg.content) {
            let current = output.last_mut().expect("just pushed");
            let arr = current["content"].as_array_mut().expect("array");
            let mut j = i + 1;
            while j < len
                && messages[j].role == Role::User
                && is_tool_result_only(&messages[j].content)
            {
                let extra = convert_content_blocks(&messages[j].content, ghost_tool_names);
                arr.extend(extra);
                j += 1;
            }
            // Skip the merged messages.
            i = j;
            continue;
        }

        i += 1;
    }

    // Flush any remaining deferred system blocks.
    if !deferred_system_blocks.is_empty() {
        output.push(json!({
            "role": "user",
            "content": deferred_system_blocks,
        }));
    }

    // --- Cache control on last user message's last content block ---
    apply_cache_control_to_last_user(&mut output);

    Ok(output)
}

/// Check whether any message from `idx` onward (up to the next assistant
/// message) is a user message whose tool_result IDs cover at least one of
/// the given `tool_use_ids`.
///
/// Skips system messages so that injected notifications (e.g. from
/// `send-image`) between assistant tool_use and user tool_result don't
/// cause false orphan detection.
fn has_matching_tool_results(
    messages: &[ChatMessage],
    idx: usize,
    tool_use_ids: &[String],
) -> bool {
    for msg in &messages[idx..] {
        match msg.role {
            // Next assistant turn — stop scanning.
            Role::Assistant => return false,
            // System messages (e.g. "[sent image: …]") can appear between
            // an assistant tool_use and its user tool_result — skip them.
            Role::System => continue,
            Role::User => {
                let found = msg.content.iter().any(|b| match b {
                    ContentBlock::ToolResult { tool_use_id, .. } => {
                        let normalized = normalize_tool_call_id(tool_use_id);
                        tool_use_ids.contains(&normalized)
                    }
                    _ => false,
                });
                if found {
                    return true;
                }
                // User message without matching results (e.g. text-only) —
                // keep scanning in case the results are in a later message.
            }
        }
    }
    false
}

/// Returns true if all blocks in the message are `ToolResult`.
fn is_tool_result_only(content: &[ContentBlock]) -> bool {
    !content.is_empty()
        && content
            .iter()
            .all(|b| matches!(b, ContentBlock::ToolResult { .. }))
}

/// Add `cache_control: ephemeral` to the last content block of the last
/// user message in the output array.
fn apply_cache_control_to_last_user(messages: &mut [Value]) {
    if let Some(last_user) = messages.iter_mut().rev().find(|m| m["role"] == "user")
        && let Some(arr) = last_user["content"].as_array_mut()
        && let Some(last_block) = arr.last_mut()
    {
        last_block["cache_control"] = json!({"type": "ephemeral"});
    }
}

// ---------------------------------------------------------------------------
// Content block conversion
// ---------------------------------------------------------------------------

/// Convert a slice of Ghost `ContentBlock`s into Anthropic JSON blocks.
fn convert_content_blocks(blocks: &[ContentBlock], ghost_tool_names: &[&str]) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    let mut has_text = false;
    let mut has_image = false;

    for block in blocks {
        match block {
            ContentBlock::Text { text } => {
                let sanitized = sanitize_surrogates(text);
                if sanitized.is_empty() {
                    continue;
                }
                has_text = true;
                out.push(json!({"type": "text", "text": sanitized}));
            }
            ContentBlock::ToolUse { id, name, input } => {
                let translated = translate_tool_name(name, ghost_tool_names);
                out.push(json!({
                    "type": "tool_use",
                    "id": normalize_tool_call_id(id),
                    "name": translated,
                    "input": input,
                }));
            }
            ContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                out.push(json!({
                    "type": "tool_result",
                    "tool_use_id": normalize_tool_call_id(tool_use_id),
                    "content": sanitize_surrogates(content),
                    "is_error": is_error,
                }));
            }
            ContentBlock::Image {
                path,
                mime_type,
                filename: _,
            } => {
                has_image = true;
                match std::fs::read(path) {
                    Ok(bytes) => {
                        let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
                        out.push(json!({
                            "type": "image",
                            "source": {
                                "type": "base64",
                                "media_type": mime_type,
                                "data": encoded,
                            }
                        }));
                    }
                    Err(e) => {
                        tracing::warn!(
                            path = %path,
                            error = %e,
                            "Failed to read image file, skipping"
                        );
                    }
                }
            }
            ContentBlock::Thinking {
                text,
                signature,
                opaque_data,
            } => {
                if let (Some(text), Some(sig)) = (text, signature) {
                    // Anthropic thinking: reconstruct with signature.
                    out.push(json!({
                        "type": "thinking",
                        "thinking": text,
                        "signature": sig,
                    }));
                } else if opaque_data.is_some() && text.is_none() && signature.is_none() {
                    // Anthropic redacted_thinking: no readable text,
                    // has opaque blob. Only reconstruct as
                    // redacted_thinking when there's NO text and NO
                    // signature -- this distinguishes it from Codex
                    // reasoning (which has text + opaque_data but no
                    // signature).
                    out.push(json!({
                        "type": "redacted_thinking",
                        "data": opaque_data.as_ref().unwrap(),
                    }));
                } else if let Some(text) = text {
                    // Cross-model block (e.g. Codex reasoning sent to
                    // Anthropic). Convert readable text to a plain text
                    // note so context survives.
                    out.push(json!({
                        "type": "text",
                        "text": format!("[Reasoning]: {text}"),
                    }));
                    has_text = true;
                }
                // If nothing matched (all None), skip silently.
            }
            ContentBlock::RawOutput { .. } => {
                // RawOutput types (e.g. function_call fallbacks) are
                // not Anthropic-native -- skip silently.
            }
        }
    }

    // Thinking blocks must precede other content per Anthropic API.
    let mut thinking = Vec::new();
    let mut rest = Vec::new();
    for block in out {
        let block_type = block.get("type").and_then(Value::as_str).unwrap_or("");
        if block_type == "thinking" || block_type == "redacted_thinking" {
            thinking.push(block);
        } else {
            rest.push(block);
        }
    }
    thinking.extend(rest);
    let mut out = thinking;

    // Image-only messages: prepend a text placeholder.
    if has_image && !has_text {
        out.insert(0, json!({"type": "text", "text": "[Image attached]"}));
    }

    out
}

/// Translate a tool name: if it's a Ghost tool name, map through
/// `to_claude_code_name`. Otherwise pass through.
fn translate_tool_name(name: &str, _ghost_tool_names: &[&str]) -> String {
    to_claude_code_name(name)
}

fn role_str(role: &Role) -> &'static str {
    match role {
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::System => "system",
    }
}

// ---------------------------------------------------------------------------
// Thinking config
// ---------------------------------------------------------------------------

/// Apply thinking configuration to the request body. Returns true if
/// thinking was enabled (so temperature should be omitted).
fn apply_thinking_config(request: &ChatRequest, body: &mut Value) -> bool {
    let effort = match request.reasoning_effort {
        Some(ReasoningEffort::None) | None => return false,
        Some(e) => e,
    };

    let max_tokens = request.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS);

    if is_adaptive_thinking_model(&request.model) {
        body["thinking"] = json!({"type": "adaptive"});
        body["output_config"] = json!({"effort": effort.as_str()});
    } else {
        let budget = std::cmp::min(max_tokens * 2, 16000);
        body["thinking"] = json!({
            "type": "enabled",
            "budget_tokens": budget,
        });
    }

    true
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn simple_request(messages: Vec<ChatMessage>) -> ChatRequest {
        ChatRequest {
            model: "claude-sonnet-4-6-20250514".into(),
            messages,
            tools: None,
            max_tokens: Some(4096),
            temperature: Some(0.7),
            system: Some("You are helpful.".into()),
            reasoning_effort: None,
            cache_key: String::new(),
            turn_state: None,
            debug_context: None,
            text_verbosity: None,
        }
    }

    #[test]
    fn system_prompt_prepends_preamble_with_cache_control() {
        let req = simple_request(vec![ChatMessage {
            role: Role::User,
            content: vec![ContentBlock::Text { text: "hi".into() }],
        }]);
        let body = build_request_body(&req, &[]).unwrap();
        let system = body["system"].as_array().unwrap();
        assert_eq!(system.len(), 2);
        let preamble = system[0]["text"].as_str().unwrap();
        assert!(preamble.starts_with("You are Claude Code"));
        assert_eq!(system[0]["cache_control"]["type"], "ephemeral");
        let user_system = system[1]["text"].as_str().unwrap();
        assert_eq!(user_system, "You are helpful.");
        assert_eq!(system[1]["cache_control"]["type"], "ephemeral");
    }

    #[test]
    fn tool_names_translated_in_definitions() {
        let req = ChatRequest {
            tools: Some(vec![ToolDefinition {
                name: "read".into(),
                description: "Read a file".into(),
                input_schema: json!({"type": "object"}),
            }]),
            ..simple_request(vec![ChatMessage {
                role: Role::User,
                content: vec![ContentBlock::Text { text: "hi".into() }],
            }])
        };
        let body = build_request_body(&req, &["read"]).unwrap();
        assert_eq!(body["tools"][0]["name"], "Read");
    }

    #[test]
    fn tool_use_in_history_gets_translated_name_and_normalized_id() {
        let req = simple_request(vec![
            ChatMessage {
                role: Role::User,
                content: vec![ContentBlock::Text { text: "hi".into() }],
            },
            ChatMessage {
                role: Role::Assistant,
                content: vec![ContentBlock::ToolUse {
                    id: "a|b|c".into(),
                    name: "read".into(),
                    input: json!({}),
                }],
            },
            ChatMessage {
                role: Role::User,
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "a|b|c".into(),
                    content: "file contents".into(),
                    is_error: false,
                }],
            },
        ]);
        let body = build_request_body(&req, &["read"]).unwrap();
        let messages = body["messages"].as_array().unwrap();
        let tool_use = &messages[1]["content"][0];
        assert_eq!(tool_use["name"], "Read");
        assert_eq!(tool_use["id"], "a_b_c");
        let tool_result = &messages[2]["content"][0];
        assert_eq!(tool_result["tool_use_id"], "a_b_c");
    }

    #[test]
    fn consecutive_tool_results_batched() {
        let req = simple_request(vec![
            ChatMessage {
                role: Role::User,
                content: vec![ContentBlock::Text { text: "hi".into() }],
            },
            ChatMessage {
                role: Role::Assistant,
                content: vec![
                    ContentBlock::ToolUse {
                        id: "1".into(),
                        name: "a".into(),
                        input: json!({}),
                    },
                    ContentBlock::ToolUse {
                        id: "2".into(),
                        name: "b".into(),
                        input: json!({}),
                    },
                ],
            },
            ChatMessage {
                role: Role::User,
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "1".into(),
                    content: "r1".into(),
                    is_error: false,
                }],
            },
            ChatMessage {
                role: Role::User,
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "2".into(),
                    content: "r2".into(),
                    is_error: false,
                }],
            },
        ]);
        let body = build_request_body(&req, &[]).unwrap();
        let messages = body["messages"].as_array().unwrap();
        let last_user = messages.last().unwrap();
        assert_eq!(last_user["role"], "user");
        let content = last_user["content"].as_array().unwrap();
        assert_eq!(content.len(), 2);
        assert_eq!(content[0]["type"], "tool_result");
        assert_eq!(content[1]["type"], "tool_result");
    }

    #[test]
    fn orphaned_tool_calls_get_synthetic_error_results() {
        let req = simple_request(vec![
            ChatMessage {
                role: Role::User,
                content: vec![ContentBlock::Text { text: "hi".into() }],
            },
            ChatMessage {
                role: Role::Assistant,
                content: vec![ContentBlock::ToolUse {
                    id: "orphan".into(),
                    name: "bash".into(),
                    input: json!({}),
                }],
            },
            ChatMessage {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: "continue".into(),
                }],
            },
        ]);
        let body = build_request_body(&req, &[]).unwrap();
        let messages = body["messages"].as_array().unwrap();
        // messages[0] = user "hi"
        // messages[1] = assistant tool_use
        // messages[2] = synthetic user tool_result (orphan fix)
        // messages[3] = user "continue"
        let synthetic = &messages[2];
        assert_eq!(synthetic["role"], "user");
        let content = synthetic["content"].as_array().unwrap();
        assert_eq!(content[0]["type"], "tool_result");
        assert_eq!(content[0]["is_error"], true);
    }

    #[test]
    fn temperature_omitted_when_thinking_enabled() {
        let req = ChatRequest {
            temperature: Some(0.7),
            reasoning_effort: Some(ReasoningEffort::High),
            ..simple_request(vec![ChatMessage {
                role: Role::User,
                content: vec![ContentBlock::Text { text: "hi".into() }],
            }])
        };
        let body = build_request_body(&req, &[]).unwrap();
        assert!(body.get("temperature").is_none());
        assert!(body.get("thinking").is_some());
    }

    #[test]
    fn adaptive_thinking_for_new_models() {
        let req = ChatRequest {
            model: "claude-opus-4-6-20250514".into(),
            reasoning_effort: Some(ReasoningEffort::High),
            ..simple_request(vec![ChatMessage {
                role: Role::User,
                content: vec![ContentBlock::Text { text: "hi".into() }],
            }])
        };
        let body = build_request_body(&req, &[]).unwrap();
        assert_eq!(body["thinking"]["type"], "adaptive");
        assert_eq!(body["output_config"]["effort"], "high");
    }

    #[test]
    fn budget_thinking_for_older_models() {
        let req = ChatRequest {
            model: "claude-3-5-sonnet-20241022".into(),
            reasoning_effort: Some(ReasoningEffort::High),
            ..simple_request(vec![ChatMessage {
                role: Role::User,
                content: vec![ContentBlock::Text { text: "hi".into() }],
            }])
        };
        let body = build_request_body(&req, &[]).unwrap();
        assert_eq!(body["thinking"]["type"], "enabled");
        assert!(body["thinking"]["budget_tokens"].as_u64().unwrap() > 0);
    }

    #[test]
    fn cache_control_on_last_user_message() {
        let req = simple_request(vec![
            ChatMessage {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: "first".into(),
                }],
            },
            ChatMessage {
                role: Role::Assistant,
                content: vec![ContentBlock::Text {
                    text: "reply".into(),
                }],
            },
            ChatMessage {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: "second".into(),
                }],
            },
        ]);
        let body = build_request_body(&req, &[]).unwrap();
        let messages = body["messages"].as_array().unwrap();
        let last_user = &messages[2];
        let last_block = last_user["content"].as_array().unwrap().last().unwrap();
        assert_eq!(last_block["cache_control"]["type"], "ephemeral");
        let first_block = &messages[0]["content"].as_array().unwrap()[0];
        assert!(first_block.get("cache_control").is_none());
    }

    #[test]
    fn sanitize_surrogates_strips_unpaired() {
        let input = "hello\\ud800world";
        let sanitized = sanitize_surrogates(input);
        assert!(!sanitized.contains("\\ud800"));
        assert!(sanitized.contains("hello"));
        assert!(sanitized.contains("world"));
    }

    #[test]
    fn image_only_message_gets_placeholder_text() {
        let req = simple_request(vec![ChatMessage {
            role: Role::User,
            content: vec![ContentBlock::Image {
                path: "/tmp/test.png".into(),
                mime_type: "image/png".into(),
                filename: "test.png".into(),
            }],
        }]);
        let body = build_request_body(&req, &[]).unwrap();
        let messages = body["messages"].as_array().unwrap();
        let content = messages[0]["content"].as_array().unwrap();
        assert!(content.iter().any(|b| b["type"] == "text"));
    }

    #[test]
    fn thinking_blocks_ordered_before_tool_use() {
        let req = simple_request(vec![
            ChatMessage {
                role: Role::User,
                content: vec![ContentBlock::Text { text: "hi".into() }],
            },
            ChatMessage {
                role: Role::Assistant,
                content: vec![
                    ContentBlock::ToolUse {
                        id: "t1".into(),
                        name: "bash".into(),
                        input: json!({}),
                    },
                    ContentBlock::Thinking {
                        text: Some("let me think".into()),
                        signature: Some("sig123".into()),
                        opaque_data: None,
                    },
                ],
            },
            ChatMessage {
                role: Role::User,
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "t1".into(),
                    content: "ok".into(),
                    is_error: false,
                }],
            },
        ]);
        let body = build_request_body(&req, &["bash"]).unwrap();
        let messages = body["messages"].as_array().unwrap();
        let assistant = &messages[1];
        let content = assistant["content"].as_array().unwrap();
        assert_eq!(content[0]["type"], "thinking", "thinking must come first");
        assert_eq!(
            content[1]["type"], "tool_use",
            "tool_use must come after thinking"
        );
    }

    #[test]
    fn system_role_converted_to_user_for_anthropic() {
        let req = simple_request(vec![
            ChatMessage {
                role: Role::System,
                content: vec![ContentBlock::Text {
                    text: "Summary of prior conversation.".into(),
                }],
            },
            ChatMessage {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: "hello".into(),
                }],
            },
        ]);
        let body = build_request_body(&req, &[]).unwrap();
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages[0]["role"], "user");
        assert!(
            messages[0]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("conversation summary"),
        );
    }

    /// Regression: system messages injected between assistant tool_use and
    /// user tool_result (e.g. from `ghost send-image`) must be deferred so
    /// tool_result is immediately adjacent. See: tool_use → system → tool_result.
    #[test]
    fn system_message_between_tool_use_and_result_deferred() {
        let req = simple_request(vec![
            ChatMessage {
                role: Role::User,
                content: vec![ContentBlock::Text { text: "hi".into() }],
            },
            ChatMessage {
                role: Role::Assistant,
                content: vec![ContentBlock::ToolUse {
                    id: "call_abc".into(),
                    name: "shell".into(),
                    input: json!({"command": "send-image"}),
                }],
            },
            // System message injected between tool_use and tool_result
            ChatMessage {
                role: Role::System,
                content: vec![ContentBlock::Text {
                    text: "[sent image: photo.png]".into(),
                }],
            },
            ChatMessage {
                role: Role::User,
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "call_abc".into(),
                    content: "Exit code: 0".into(),
                    is_error: false,
                }],
            },
            ChatMessage {
                role: Role::Assistant,
                content: vec![ContentBlock::Text {
                    text: "Sent.".into(),
                }],
            },
        ]);
        let body = build_request_body(&req, &[]).unwrap();
        let messages = body["messages"].as_array().unwrap();

        // messages[0] = user "hi"
        // messages[1] = assistant tool_use
        // messages[2] = user [deferred system text + tool_result]
        // messages[3] = assistant "Sent."
        assert_eq!(messages.len(), 4, "expected 4 messages, got {messages:?}");

        // The message right after the assistant tool_use must be user
        // and must contain the tool_result — Anthropic requires strict adjacency.
        let after_tool_use = &messages[2];
        assert_eq!(after_tool_use["role"], "user");
        let content = after_tool_use["content"].as_array().unwrap();

        // Deferred system text is prepended, tool_result follows.
        assert!(
            content
                .iter()
                .any(|b| b["type"] == "text"
                    && b["text"].as_str().unwrap_or("").contains("sent image")),
            "deferred system text must be in the tool_result message"
        );
        assert!(
            content
                .iter()
                .any(|b| b["type"] == "tool_result" && b["tool_use_id"] == "call_abc"),
            "tool_result must be in the same message"
        );

        // No synthetic "interrupted" results anywhere.
        for msg in messages {
            if let Some(content) = msg["content"].as_array() {
                for block in content {
                    if block["type"] == "tool_result" {
                        assert_ne!(
                            block["content"].as_str().unwrap_or(""),
                            "Tool execution was interrupted.",
                            "system message between tool_use/result caused false orphan"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn cross_model_codex_reasoning_becomes_text_not_redacted() {
        let req = simple_request(vec![
            ChatMessage {
                role: Role::User,
                content: vec![ContentBlock::Text { text: "hi".into() }],
            },
            ChatMessage {
                role: Role::Assistant,
                content: vec![
                    ContentBlock::Thinking {
                        text: Some("step by step reasoning".into()),
                        signature: None,
                        opaque_data: Some("encrypted_blob".into()),
                    },
                    ContentBlock::Text {
                        text: "answer".into(),
                    },
                ],
            },
            ChatMessage {
                role: Role::User,
                content: vec![ContentBlock::Text { text: "ok".into() }],
            },
        ]);
        let body = build_request_body(&req, &[]).unwrap();
        let messages = body["messages"].as_array().unwrap();
        let assistant = &messages[1];
        let content = assistant["content"].as_array().unwrap();
        // Should be converted to text, not redacted_thinking
        assert!(content.iter().all(|b| b["type"] != "redacted_thinking"));
        assert!(content.iter().any(|b| {
            b["type"] == "text"
                && b["text"]
                    .as_str()
                    .unwrap_or("")
                    .contains("step by step reasoning")
        }));
    }
}
