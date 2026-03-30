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

/// Default max output tokens sent to the Anthropic API (required field).
/// Set to the model-family maximum so extended thinking never starves output.
const DEFAULT_MAX_TOKENS: u32 = 128_000;
static SURROGATE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\\u[dD][89abAB][0-9a-fA-F]{2}").expect("valid regex"));

/// Strip unpaired UTF-16 surrogate escape sequences from text.
pub(crate) fn sanitize_surrogates(text: &str) -> String {
    SURROGATE_RE.replace_all(text, "").to_string()
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

    // NOTE: system messages between tool_use/tool_result pairs are
    // relocated by `relocate_system_messages_between_tool_pairs` in
    // the provider-agnostic layer (session.rs) BEFORE this function
    // is called. By the time we get here, system messages are already
    // in safe positions.

    let mut i = 0;
    while i < len {
        let msg = &messages[i];
        let role_str = match msg.role {
            // Anthropic doesn't accept system in messages array.
            Role::System => "user",
            _ => role_str(&msg.role),
        };

        let content_blocks = if msg.role == Role::System {
            msg.content
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
                .collect::<Vec<_>>()
        } else {
            convert_content_blocks(&msg.content, ghost_tool_names)
        };

        if content_blocks.is_empty() {
            i += 1;
            continue;
        }

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
                    // Insert synthetic error results for orphaned tool calls.
                    output.push(synthetic_error_results(&tool_use_ids));
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

    // --- Merge consecutive same-role messages ---
    // System messages are converted to "user" role for Anthropic, which can
    // create user-user pairs (e.g. completion watcher system message followed
    // by user trigger message). The API requires strict alternation.
    let mut merged: Vec<Value> = Vec::with_capacity(output.len());
    for msg in output {
        if let Some(last) = merged.last_mut()
            && last["role"] == msg["role"]
            && let Some(last_content) = last["content"].as_array_mut()
            && let Some(new_content) = msg["content"].as_array()
        {
            last_content.extend(new_content.iter().cloned());
        } else {
            merged.push(msg);
        }
    }
    let mut output = merged;

    // --- Cache control on last user message's last content block ---
    apply_cache_control_to_last_user(&mut output);

    Ok(output)
}

/// Build a synthetic user message with error results for each orphaned tool call.
fn synthetic_error_results(tool_use_ids: &[String]) -> Value {
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
    json!({
        "role": "user",
        "content": synthetic,
    })
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
                } else if let Some(data) = opaque_data
                    && text.is_none()
                    && signature.is_none()
                {
                    // Anthropic redacted_thinking: no readable text,
                    // has opaque blob. Only reconstruct as
                    // redacted_thinking when there's NO text and NO
                    // signature -- this distinguishes it from Codex
                    // reasoning (which has text + opaque_data but no
                    // signature).
                    out.push(json!({
                        "type": "redacted_thinking",
                        "data": data,
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

    // Nest image blocks inside tool_result when both exist in the same
    // message (e.g. file_read returning a screenshot). The Anthropic API
    // expects tool result images inside the tool_result content array,
    // not as siblings at the message level.
    if has_image && out.iter().any(|b| b["type"] == "tool_result") {
        let image_blocks: Vec<Value> = out
            .iter()
            .filter(|b| b["type"] == "image")
            .cloned()
            .collect();
        out.retain(|b| b["type"] != "image");

        if let Some(tr) = out.iter_mut().rev().find(|b| b["type"] == "tool_result") {
            let text_content = tr["content"].take();
            let mut content_arr: Vec<Value> = Vec::new();
            if let Some(text) = text_content.as_str()
                && !text.is_empty()
            {
                content_arr.push(json!({"type": "text", "text": text}));
            }
            content_arr.extend(image_blocks);
            tr["content"] = Value::Array(content_arr);
        }
        // Images are now nested; clear flag so placeholder isn't added.
        has_image = false;
    }

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

    body["thinking"] = json!({"type": "adaptive"});
    body["output_config"] = json!({"effort": effort.as_str()});

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
    fn max_tokens_defaults_when_none() {
        let req = ChatRequest {
            max_tokens: None,
            ..simple_request(vec![ChatMessage {
                role: Role::User,
                content: vec![ContentBlock::Text { text: "hi".into() }],
            }])
        };
        let body = build_request_body(&req, &[]).unwrap();
        assert_eq!(body["max_tokens"], DEFAULT_MAX_TOKENS);
    }

    #[test]
    fn max_tokens_included_when_set() {
        let req = simple_request(vec![ChatMessage {
            role: Role::User,
            content: vec![ContentBlock::Text { text: "hi".into() }],
        }]);
        let body = build_request_body(&req, &[]).unwrap();
        assert_eq!(body["max_tokens"], 4096);
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

    /// After provider-agnostic relocation, system messages that were
    /// between tool_use/tool_result have been moved. The Anthropic
    /// converter receives already-clean input. This validates that
    /// the relocated structure (system text merged into a later user
    /// message) is accepted by the converter.
    #[test]
    fn relocated_system_text_in_user_message_after_tool_result() {
        // This is the shape AFTER relocate_system_messages_between_tool_pairs:
        // the system text has been merged into the next user text message.
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
            ChatMessage {
                role: Role::User,
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "call_abc".into(),
                    content: "Exit code: 0".into(),
                    is_error: false,
                }],
            },
            // System text was relocated here by the provider-agnostic layer,
            // merged into the assistant's following text response is not possible
            // so it became a standalone system message before the assistant.
            ChatMessage {
                role: Role::System,
                content: vec![ContentBlock::Text {
                    text: "[sent image: photo.png]".into(),
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

        // After the merge fix, the system-as-user text is merged into
        // the preceding user message (the tool_result message), so:
        // messages[0] = user "hi"
        // messages[1] = assistant tool_use
        // messages[2] = user [tool_result, system text]
        // messages[3] = assistant "Sent."
        assert_eq!(messages.len(), 4, "got {messages:?}");
        assert_eq!(messages[2]["role"], "user");
        let content = messages[2]["content"].as_array().unwrap();
        assert!(
            content.iter().any(|b| b["type"] == "tool_result"),
            "merged user message must contain the tool_result block"
        );
        assert!(
            content.iter().any(|b| {
                b["type"] == "text" && b["text"].as_str().unwrap_or("").contains("sent image")
            }),
            "merged user message must contain the relocated system text"
        );

        // No consecutive same-role messages.
        for i in 1..messages.len() {
            assert_ne!(
                messages[i - 1]["role"],
                messages[i]["role"],
                "consecutive same-role messages at {}/{i}",
                i - 1,
            );
        }

        // No synthetic "interrupted" results anywhere.
        let tool_result_blocks = messages
            .iter()
            .filter_map(|msg| msg["content"].as_array())
            .flatten()
            .filter(|block| block["type"] == "tool_result");
        for block in tool_result_blocks {
            assert_ne!(
                block["content"].as_str().unwrap_or(""),
                "Tool execution was interrupted.",
            );
        }
    }

    /// Bug reproduction: system messages converted to user role create
    /// consecutive user-user messages, violating the Anthropic API's
    /// alternation requirement. This happens when the completion watcher
    /// injects a system message (agent result) followed by a user trigger
    /// message, or when compaction summaries precede user messages.
    #[test]
    fn no_consecutive_same_role_messages_after_system_conversion() {
        let req = simple_request(vec![
            ChatMessage {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: "hello".into(),
                }],
            },
            ChatMessage {
                role: Role::Assistant,
                content: vec![ContentBlock::Text {
                    text: "hi there".into(),
                }],
            },
            // System message (e.g. agent completion result, compaction summary)
            // — gets converted to role "user" for Anthropic
            ChatMessage {
                role: Role::System,
                content: vec![ContentBlock::Text {
                    text: "[agent:deep-research completed]\n{\"findings\": \"...\"}".into(),
                }],
            },
            // User trigger from event handler — also role "user"
            ChatMessage {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: "[system] Background task completed.".into(),
                }],
            },
        ]);
        let body = build_request_body(&req, &[]).unwrap();
        let messages = body["messages"].as_array().unwrap();

        for i in 1..messages.len() {
            let prev_role = messages[i - 1]["role"].as_str().unwrap();
            let curr_role = messages[i]["role"].as_str().unwrap();
            assert_ne!(
                prev_role,
                curr_role,
                "consecutive '{prev_role}' messages at positions {} and {}: \
                 Anthropic API requires strict user/assistant alternation",
                i - 1,
                i,
            );
        }
    }

    /// Bug reproduction: when file_read returns an image, the Image
    /// content block ends up as a sibling to ToolResult at the message
    /// level. The Anthropic API expects images from tool results to be
    /// nested inside the tool_result's content array.
    #[test]
    fn tool_result_image_nested_inside_result_not_sibling() {
        // Create a minimal test file (just needs to be readable).
        let tmp = std::env::temp_dir().join("ghost_test_tool_result_image.bin");
        std::fs::write(&tmp, b"fake-image-data").unwrap();

        let req = simple_request(vec![
            ChatMessage {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: "read the screenshot".into(),
                }],
            },
            ChatMessage {
                role: Role::Assistant,
                content: vec![ContentBlock::ToolUse {
                    id: "tool1".into(),
                    name: "file_read".into(),
                    input: json!({"path": tmp.to_str().unwrap()}),
                }],
            },
            // This is the shape produced by execute_single_tool for an image:
            // ToolResult + Image as siblings in the same user message.
            ChatMessage {
                role: Role::User,
                content: vec![
                    ContentBlock::ToolResult {
                        tool_use_id: "tool1".into(),
                        content: format!("Image file: {}", tmp.display()),
                        is_error: false,
                    },
                    ContentBlock::Image {
                        path: tmp.to_str().unwrap().into(),
                        mime_type: "image/png".into(),
                        filename: "screenshot.png".into(),
                    },
                ],
            },
        ]);
        let body = build_request_body(&req, &[]).unwrap();
        let messages = body["messages"].as_array().unwrap();
        let last_user = messages.last().unwrap();
        assert_eq!(last_user["role"], "user");
        let content = last_user["content"].as_array().unwrap();

        // The image must NOT be a sibling at the message content level.
        // It should be nested inside the tool_result's content array.
        let has_image_sibling = content.iter().any(|b| b["type"] == "image");
        assert!(
            !has_image_sibling,
            "image block is a sibling to tool_result at message level; \
             should be nested inside the tool_result content array"
        );

        // The tool_result's content should be an array containing
        // both the text and the image, not a plain string.
        let tool_result = content
            .iter()
            .find(|b| b["type"] == "tool_result")
            .expect("tool_result block must exist");
        assert!(
            tool_result["content"].is_array(),
            "tool_result content should be an array (text + image), \
             not a string: got {:?}",
            tool_result["content"],
        );

        // No spurious [Image attached] text block either.
        let has_image_attached = content
            .iter()
            .any(|b| b["type"] == "text" && b["text"] == "[Image attached]");
        assert!(
            !has_image_attached,
            "[Image attached] placeholder should not appear when \
             image is part of a tool result"
        );

        let _ = std::fs::remove_file(&tmp);
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
