use std::path::Path;

use serde_json::{Value, json};
use surrealdb::sql::Thing;

use crate::db;
use crate::providers::{ChatMessage, ContentBlock, Role};
use crate::tools::ToolError;

use super::types::{ChatError, Citation, StructuredResponse};

pub(super) fn parse_session_thing(session_id: &str) -> Result<Thing, ChatError> {
    if session_id.contains(':') {
        let mut parts = session_id.splitn(2, ':');
        let table = parts.next().unwrap_or_default();
        let id = parts.next().unwrap_or_default();
        if table.is_empty() || id.is_empty() {
            return Err(ChatError::InvalidSessionId {
                session_id: session_id.to_string(),
            });
        }
        return Ok(Thing::from((table, id)));
    }

    if session_id.trim().is_empty() {
        return Err(ChatError::InvalidSessionId {
            session_id: session_id.to_string(),
        });
    }

    Ok(Thing::from(("session", session_id)))
}

pub(super) fn convert_stored_message_to_provider_message(
    message: db::sessions::MessageRecord,
) -> ChatMessage {
    let role = match message.role.as_str() {
        "user" => Role::User,
        "assistant" => Role::Assistant,
        _ => Role::System,
    };
    let mut content = Vec::new();
    if !message.content.trim().is_empty() {
        content.push(ContentBlock::Text {
            text: message.content,
        });
    }
    if let Some(tool_calls) = message.tool_calls {
        for call in tool_calls {
            if let (Some(id), Some(name)) = (
                call.get("id").and_then(Value::as_str),
                call.get("name").and_then(Value::as_str),
            ) {
                content.push(ContentBlock::ToolUse {
                    id: id.to_string(),
                    name: name.to_string(),
                    input: call.get("input").cloned().unwrap_or_else(|| json!({})),
                });
            }
        }
    }
    if let Some(tool_results) = message.tool_results {
        for result in tool_results {
            if let Some(tool_use_id) = result.get("tool_use_id").and_then(Value::as_str) {
                content.push(ContentBlock::ToolResult {
                    tool_use_id: tool_use_id.to_string(),
                    content: result
                        .get("content")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    is_error: result
                        .get("is_error")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                });
            }
        }
    }
    if let Some(raw_output) = message.raw_output {
        for item in raw_output {
            if let Some(original_type) = item.get("original_type").and_then(Value::as_str) {
                let value = item.get("value").cloned().unwrap_or(Value::Null);
                content.push(ContentBlock::RawOutput {
                    original_type: original_type.to_string(),
                    value,
                });
            }
        }
    }
    ChatMessage { role, content }
}

pub(super) fn extract_tool_use_blocks(content: &[ContentBlock]) -> Vec<Value> {
    content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::ToolUse { id, name, input } => Some(json!({
                "id": id,
                "name": name,
                "input": input
            })),
            _ => None,
        })
        .collect()
}

pub(super) fn extract_text_content(content: &[ContentBlock]) -> String {
    content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

pub(super) fn extract_latest_assistant_text(history: &[ChatMessage]) -> String {
    history
        .iter()
        .rev()
        .find_map(|msg| {
            if msg.role == Role::Assistant {
                Some(extract_text_content(&msg.content))
            } else {
                None
            }
        })
        .unwrap_or_default()
}

/// Check if any tool call in `tool_uses` is the `respond` output tool.
/// Returns `Some((message, citations))` if found, `None` otherwise.
pub(super) fn parse_respond_call(
    respond_name: &str,
    tool_uses: &[Value],
) -> Option<(String, Vec<Citation>)> {
    for call in tool_uses {
        let name = call.get("name").and_then(Value::as_str)?;
        if name != respond_name {
            continue;
        }
        let input = call.get("input")?;
        let parsed: StructuredResponse = match serde_json::from_value(input.clone()) {
            Ok(v) => v,
            Err(e) => {
                logfire::warn!(
                    "respond tool arguments failed to parse",
                    error = e.to_string(),
                );
                return None;
            }
        };
        let citations = parsed
            .citations
            .into_iter()
            .map(|c| Citation {
                source: c.source,
                url: None,
                context: c.context,
            })
            .collect();
        return Some((parsed.message, citations));
    }
    None
}

pub(super) fn citations_to_values(citations: &[Citation]) -> Vec<Value> {
    citations
        .iter()
        .map(|citation| {
            json!({
                "source": citation.source,
                "url": citation.url,
                "context": citation.context
            })
        })
        .collect()
}

pub(super) fn raw_output_to_values(content: &[ContentBlock]) -> Option<Vec<Value>> {
    let values: Vec<Value> = content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::RawOutput {
                original_type,
                value,
            } => Some(json!({
                "original_type": original_type,
                "value": value,
            })),
            _ => None,
        })
        .collect();
    if values.is_empty() {
        None
    } else {
        Some(values)
    }
}

pub(super) fn tool_results_to_values(results: &[ContentBlock]) -> Vec<Value> {
    results
        .iter()
        .filter_map(|result| match result {
            ContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => Some(json!({
                "tool_use_id": tool_use_id,
                "content": content,
                "is_error": is_error
            })),
            _ => None,
        })
        .collect()
}

pub(super) fn resolve_web_cache_url(workspace: &Path, source: &str) -> Option<String> {
    let path = workspace.join(source);
    let content = std::fs::read_to_string(path).ok()?;
    let mut lines = content.lines();
    if lines.next()? != "---" {
        return None;
    }
    for line in lines {
        if line == "---" {
            break;
        }
        if let Some(value) = line.strip_prefix("url:") {
            let url = value.trim().trim_matches('"');
            if !url.is_empty() {
                return Some(url.to_string());
            }
        }
        if let Some(value) = line.strip_prefix("source_url:") {
            let url = value.trim().trim_matches('"');
            if !url.is_empty() {
                return Some(url.to_string());
            }
        }
    }
    None
}

pub(super) fn render_tool_error(error: ToolError) -> String {
    match error {
        ToolError::NotFound { name } => format!("Tool not found: {name}"),
        ToolError::InvalidParams(msg) => format!("Invalid parameters: {msg}"),
        ToolError::ExecutionFailed(msg) => format!("Execution failed: {msg}"),
        ToolError::PermissionDenied(msg) => {
            format!("Permission denied: {msg}")
        }
    }
}
