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

pub(super) fn parse_structured_or_fallback(content: &[ContentBlock]) -> (String, Vec<Citation>) {
    let text = extract_text_content(content);
    let parsed = serde_json::from_str::<StructuredResponse>(&text);
    match parsed {
        Ok(structured) => (
            structured.message,
            structured
                .citations
                .into_iter()
                .map(|citation| Citation {
                    source: citation.source,
                    url: None,
                    context: citation.context,
                })
                .collect(),
        ),
        Err(e) => {
            let looks_like_json =
                text.trim_start().starts_with('{') && text.trim_end().ends_with('}');
            if looks_like_json {
                logfire::warn!(
                    "structured response looks like JSON but failed to parse",
                    error = e.to_string(),
                    text_prefix = text.chars().take(300).collect::<String>(),
                );
            } else {
                logfire::debug!(
                    "structured response parse failed, using raw text",
                    error = e.to_string(),
                    text_len = text.len() as u64,
                );
            }
            (text, Vec::new())
        }
    }
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

pub(super) fn citation_response_format() -> crate::providers::ResponseFormat {
    crate::providers::ResponseFormat::JsonSchema {
        name: "ghost_citation_response".to_string(),
        schema: json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "message": { "type": "string", "description": "The response to the OPERATOR" },
                "citations": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "additionalProperties": false,
                        "properties": {
                            "source": { "type": "string", "description": "File path or URL" },
                            "context": { "type": "string", "description": "What this source was used for" }
                        },
                        "required": ["source", "context"]
                    }
                }
            },
            "required": ["message", "citations"]
        }),
    }
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
