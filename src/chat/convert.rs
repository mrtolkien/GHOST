use serde_json::{Value, json};

use crate::db;
use crate::providers::{ChatMessage, ContentBlock, Role};
use crate::tools::ToolError;

use super::types::ChatError;

pub(super) fn parse_session_thing(session_id: &str) -> Result<String, ChatError> {
    if session_id.contains(':') {
        let mut parts = session_id.splitn(2, ':');
        let _table = parts.next().unwrap_or_default();
        let id = parts.next().unwrap_or_default();
        if id.is_empty() {
            return Err(ChatError::InvalidSessionId {
                session_id: session_id.to_string(),
            });
        }
        return Ok(id.to_string());
    }

    if session_id.trim().is_empty() {
        return Err(ChatError::InvalidSessionId {
            session_id: session_id.to_string(),
        });
    }

    Ok(session_id.to_string())
}

pub(super) fn convert_stored_message_to_provider_message(
    message: db::sessions::MessageRecord,
) -> ChatMessage {
    let role = match message.role.as_str() {
        "user" => Role::User,
        "assistant" => Role::Assistant,
        _ => Role::System,
    };
    // Parse JSON fields before moving message.content
    let tool_calls = message.tool_calls_parsed();
    let tool_results = message.tool_results_parsed();
    let raw_output = message.raw_output_parsed();
    let images = message.images_parsed();

    let mut content = Vec::new();
    if !message.content.trim().is_empty() {
        content.push(ContentBlock::Text {
            text: message.content,
        });
    }
    if let Some(images) = images {
        for img in images {
            if let (Some(path), Some(mime_type), Some(filename)) = (
                img.get("path").and_then(Value::as_str),
                img.get("mime_type").and_then(Value::as_str),
                img.get("filename").and_then(Value::as_str),
            ) {
                content.push(ContentBlock::Image {
                    path: path.to_string(),
                    mime_type: mime_type.to_string(),
                    filename: filename.to_string(),
                });
            }
        }
    }
    if let Some(tool_calls) = tool_calls {
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
    if let Some(tool_results) = tool_results {
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
    if let Some(raw_output) = raw_output {
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

pub(super) fn images_to_values(content: &[ContentBlock]) -> Option<Vec<Value>> {
    let values: Vec<Value> = content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Image {
                path,
                mime_type,
                filename,
            } => Some(json!({
                "path": path,
                "mime_type": mime_type,
                "filename": filename,
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
