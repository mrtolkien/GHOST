use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use surrealdb::sql::Thing;

use crate::db;
use crate::providers::ToolDefinition;

use super::context::ToolContext;
use super::error::ToolError;
use super::manager::Tool;

// -------------------------------------------------------------------------
// Types
// -------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    Pending,
    InProgress,
    Done,
    Skipped,
}

impl TodoStatus {
    #[must_use]
    pub fn symbol(&self) -> &'static str {
        match self {
            Self::Pending => "○",
            Self::InProgress => "◉",
            Self::Done => "✓",
            Self::Skipped => "–",
        }
    }

    fn from_str(s: &str) -> Result<Self, ToolError> {
        match s {
            "pending" => Ok(Self::Pending),
            "in_progress" => Ok(Self::InProgress),
            "done" => Ok(Self::Done),
            "skipped" => Ok(Self::Skipped),
            _ => Err(ToolError::InvalidParams(format!(
                "invalid status '{s}': must be one of \
                 pending, in_progress, done, skipped"
            ))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub status: TodoStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Format the TODO list for tool output (returned to the model).
#[must_use]
pub fn format_todo_list(items: &[TodoItem]) -> String {
    if items.is_empty() {
        return "TODO list is empty.".to_string();
    }

    let done_count = items
        .iter()
        .filter(|i| matches!(i.status, TodoStatus::Done | TodoStatus::Skipped))
        .count();
    let total = items.len();

    let mut out = format!("TODO [{done_count}/{total}]\n");
    for (i, item) in items.iter().enumerate() {
        let idx = i + 1;
        let sym = item.status.symbol();
        out.push_str(&format!("{idx}. {sym} {}", item.title));
        if let Some(desc) = &item.description {
            out.push_str(&format!(" — {desc}"));
        }
        if let Some(note) = &item.note {
            out.push_str(&format!(" [{note}]"));
        }
        out.push('\n');
    }
    out
}

/// Format the TODO list for injection into chat context (system message).
#[must_use]
pub fn format_todo_injection(items: &[TodoItem]) -> String {
    let done_count = items
        .iter()
        .filter(|i| matches!(i.status, TodoStatus::Done | TodoStatus::Skipped))
        .count();
    let total = items.len();

    let mut out = format!("Current TODO [{done_count}/{total}]:\n");
    for (i, item) in items.iter().enumerate() {
        let idx = i + 1;
        let sym = item.status.symbol();
        out.push_str(&format!("{idx}. {sym} {}", item.title));
        if let Some(note) = &item.note {
            out.push_str(&format!(" [{note}]"));
        }
        out.push('\n');
    }
    out
}

// -------------------------------------------------------------------------
// Tool implementation
// -------------------------------------------------------------------------

pub struct Todo;

#[async_trait]
impl Tool for Todo {
    fn name(&self) -> &str {
        "todo"
    }

    fn schema(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Manage a TODO list for the current session. Use \
                'plan' to replace the entire list, 'add' to append an item, \
                'update' to change an item's status, 'batch_update' to change \
                multiple items at once, or 'clear' to reset."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["plan", "add", "update", "batch_update", "clear"],
                        "description": "The action to perform"
                    },
                    "items": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "title": { "type": "string" },
                                "description": { "type": "string" }
                            },
                            "required": ["title"]
                        },
                        "description": "For 'plan': the items to create"
                    },
                    "title": {
                        "type": "string",
                        "description": "For 'add': item title"
                    },
                    "description": {
                        "type": "string",
                        "description": "For 'add': item description"
                    },
                    "index": {
                        "type": "integer",
                        "description": "For 'update': 1-based item index"
                    },
                    "status": {
                        "type": "string",
                        "enum": ["pending", "in_progress", "done", "skipped"],
                        "description": "For 'update': new status"
                    },
                    "note": {
                        "type": "string",
                        "description": "For 'update': optional note"
                    },
                    "updates": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "index": { "type": "integer" },
                                "status": {
                                    "type": "string",
                                    "enum": ["pending", "in_progress", "done", "skipped"]
                                },
                                "note": { "type": "string" }
                            },
                            "required": ["index", "status"]
                        },
                        "description": "For 'batch_update': array of updates"
                    }
                },
                "required": ["action"]
            }),
        }
    }

    #[tracing::instrument(skip_all, fields(tool = "todo"))]
    async fn execute(&self, params: Value, ctx: &ToolContext) -> Result<String, ToolError> {
        let action = params
            .get("action")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ToolError::InvalidParams("missing required parameter 'action'".to_string())
            })?;

        let session_thing = parse_session_thing(&ctx.session_id)?;

        match action {
            "plan" => self.action_plan(&params, ctx, &session_thing).await,
            "add" => self.action_add(&params, ctx, &session_thing).await,
            "update" => self.action_update(&params, ctx, &session_thing).await,
            "batch_update" => self.action_batch_update(&params, ctx, &session_thing).await,
            "clear" => self.action_clear(ctx, &session_thing).await,
            _ => Err(ToolError::InvalidParams(format!(
                "unknown action '{action}': must be one of \
                 plan, add, update, batch_update, clear"
            ))),
        }
    }
}

impl Todo {
    async fn load_list(
        &self,
        ctx: &ToolContext,
        session_id: &Thing,
    ) -> Result<Vec<TodoItem>, ToolError> {
        Ok(db::sessions::get_session_todo_list(&ctx.db, session_id)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to load TODO list: {e}")))?
            .unwrap_or_default())
    }

    async fn save_list(
        &self,
        ctx: &ToolContext,
        session_id: &Thing,
        items: &[TodoItem],
    ) -> Result<(), ToolError> {
        let list = if items.is_empty() { None } else { Some(items) };
        db::sessions::set_session_todo_list(&ctx.db, session_id, list)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to save TODO list: {e}")))
    }

    async fn action_plan(
        &self,
        params: &Value,
        ctx: &ToolContext,
        session_id: &Thing,
    ) -> Result<String, ToolError> {
        let items_val = params.get("items").ok_or_else(|| {
            ToolError::InvalidParams("'plan' requires an 'items' array".to_string())
        })?;

        let raw_items: Vec<Value> = items_val
            .as_array()
            .ok_or_else(|| ToolError::InvalidParams("'items' must be an array".to_string()))?
            .clone();

        let items: Vec<TodoItem> = raw_items
            .into_iter()
            .map(|v| {
                let title = v
                    .get("title")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        ToolError::InvalidParams("each item requires a 'title'".to_string())
                    })?
                    .to_string();
                let description = v
                    .get("description")
                    .and_then(Value::as_str)
                    .map(ToString::to_string);
                Ok(TodoItem {
                    title,
                    description,
                    status: TodoStatus::Pending,
                    note: None,
                })
            })
            .collect::<Result<Vec<_>, ToolError>>()?;

        self.save_list(ctx, session_id, &items).await?;
        Ok(format_todo_list(&items))
    }

    async fn action_add(
        &self,
        params: &Value,
        ctx: &ToolContext,
        session_id: &Thing,
    ) -> Result<String, ToolError> {
        let title = params
            .get("title")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::InvalidParams("'add' requires a 'title'".to_string()))?
            .to_string();

        let description = params
            .get("description")
            .and_then(Value::as_str)
            .map(ToString::to_string);

        let mut items = self.load_list(ctx, session_id).await?;
        items.push(TodoItem {
            title,
            description,
            status: TodoStatus::Pending,
            note: None,
        });

        self.save_list(ctx, session_id, &items).await?;
        Ok(format_todo_list(&items))
    }

    async fn action_update(
        &self,
        params: &Value,
        ctx: &ToolContext,
        session_id: &Thing,
    ) -> Result<String, ToolError> {
        let index = params.get("index").and_then(Value::as_u64).ok_or_else(|| {
            ToolError::InvalidParams("'update' requires an 'index' (1-based)".to_string())
        })? as usize;

        let status_str = params
            .get("status")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::InvalidParams("'update' requires a 'status'".to_string()))?;
        let status = TodoStatus::from_str(status_str)?;
        let note = params
            .get("note")
            .and_then(Value::as_str)
            .map(ToString::to_string);

        let mut items = self.load_list(ctx, session_id).await?;

        if index == 0 || index > items.len() {
            return Err(ToolError::InvalidParams(format!(
                "index {index} is out of range (1-{})",
                items.len()
            )));
        }

        let item = &mut items[index - 1];
        item.status = status;
        if note.is_some() {
            item.note = note;
        }

        self.save_list(ctx, session_id, &items).await?;
        Ok(format_todo_list(&items))
    }

    async fn action_batch_update(
        &self,
        params: &Value,
        ctx: &ToolContext,
        session_id: &Thing,
    ) -> Result<String, ToolError> {
        let updates = params
            .get("updates")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                ToolError::InvalidParams("'batch_update' requires an 'updates' array".to_string())
            })?;

        let mut items = self.load_list(ctx, session_id).await?;

        for update in updates {
            let index = update.get("index").and_then(Value::as_u64).ok_or_else(|| {
                ToolError::InvalidParams("each update requires an 'index'".to_string())
            })? as usize;

            let status_str = update
                .get("status")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    ToolError::InvalidParams("each update requires a 'status'".to_string())
                })?;
            let status = TodoStatus::from_str(status_str)?;
            let note = update
                .get("note")
                .and_then(Value::as_str)
                .map(ToString::to_string);

            if index == 0 || index > items.len() {
                return Err(ToolError::InvalidParams(format!(
                    "index {index} is out of range (1-{})",
                    items.len()
                )));
            }

            let item = &mut items[index - 1];
            item.status = status;
            if note.is_some() {
                item.note = note;
            }
        }

        self.save_list(ctx, session_id, &items).await?;
        Ok(format_todo_list(&items))
    }

    async fn action_clear(
        &self,
        ctx: &ToolContext,
        session_id: &Thing,
    ) -> Result<String, ToolError> {
        self.save_list(ctx, session_id, &[]).await?;
        Ok("TODO list cleared.".to_string())
    }
}

fn parse_session_thing(session_id: &str) -> Result<Thing, ToolError> {
    if session_id.contains(':') {
        let mut parts = session_id.splitn(2, ':');
        let table = parts.next().unwrap_or_default();
        let id = parts.next().unwrap_or_default();
        if table.is_empty() || id.is_empty() {
            return Err(ToolError::InvalidParams(format!(
                "invalid session ID: '{session_id}'"
            )));
        }
        return Ok(Thing::from((table, id)));
    }
    Ok(Thing::from(("session", session_id)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn todo_status_symbols() {
        assert_eq!(TodoStatus::Pending.symbol(), "○");
        assert_eq!(TodoStatus::InProgress.symbol(), "◉");
        assert_eq!(TodoStatus::Done.symbol(), "✓");
        assert_eq!(TodoStatus::Skipped.symbol(), "–");
    }

    #[test]
    fn format_todo_list_empty() {
        assert_eq!(format_todo_list(&[]), "TODO list is empty.");
    }

    #[test]
    fn format_todo_list_with_items() {
        let items = vec![
            TodoItem {
                title: "First task".to_string(),
                description: Some("details".to_string()),
                status: TodoStatus::Done,
                note: None,
            },
            TodoItem {
                title: "Second task".to_string(),
                description: None,
                status: TodoStatus::Pending,
                note: Some("blocked".to_string()),
            },
        ];
        let output = format_todo_list(&items);
        assert!(output.contains("TODO [1/2]"));
        assert!(output.contains("1. ✓ First task — details"));
        assert!(output.contains("2. ○ Second task [blocked]"));
    }

    #[test]
    fn format_todo_injection_with_items() {
        let items = vec![TodoItem {
            title: "Do something".to_string(),
            description: None,
            status: TodoStatus::InProgress,
            note: None,
        }];
        let output = format_todo_injection(&items);
        assert!(output.contains("Current TODO [0/1]"));
        assert!(output.contains("1. ◉ Do something"));
    }

    #[test]
    fn todo_status_from_str() {
        assert_eq!(
            TodoStatus::from_str("pending").unwrap(),
            TodoStatus::Pending
        );
        assert_eq!(
            TodoStatus::from_str("in_progress").unwrap(),
            TodoStatus::InProgress
        );
        assert_eq!(TodoStatus::from_str("done").unwrap(), TodoStatus::Done);
        assert_eq!(
            TodoStatus::from_str("skipped").unwrap(),
            TodoStatus::Skipped
        );
        assert!(TodoStatus::from_str("invalid").is_err());
    }

    #[test]
    fn todo_item_serialization_roundtrip() {
        let item = TodoItem {
            title: "Test".to_string(),
            description: Some("desc".to_string()),
            status: TodoStatus::InProgress,
            note: Some("wip".to_string()),
        };
        let json = serde_json::to_string(&item).unwrap();
        let deserialized: TodoItem = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.title, "Test");
        assert_eq!(deserialized.status, TodoStatus::InProgress);
        assert_eq!(deserialized.note.as_deref(), Some("wip"));
    }
}
