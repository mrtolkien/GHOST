use async_trait::async_trait;
use serde_json::{Value, json};

use crate::db;
use crate::knowledge::reconcile::reconcile_edges;
use crate::knowledge::{self, NoteFrontMatter, extract_wiki_links};
use crate::providers::ToolDefinition;

use super::context::ToolContext;
use super::error::ToolError;
use super::manager::Tool;

pub struct NoteWrite;

#[async_trait]
impl Tool for NoteWrite {
    fn name(&self) -> &str {
        "note_write"
    }

    fn schema(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Create or update a knowledge note. \
                Notes are stored as markdown files with TOML frontmatter \
                and indexed in the knowledge graph."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["create", "update"],
                        "description": "Whether to create a new note or update an existing one"
                    },
                    "title": {
                        "type": "string",
                        "description": "Note title (used as filename slug and graph node name)"
                    },
                    "body": {
                        "type": "string",
                        "description": "Markdown body content. May contain [[WikiLinks]] and [[rel>Target]] typed links."
                    },
                    "archetype": {
                        "type": "string",
                        "enum": [
                            "person", "concept", "decision", "event",
                            "place", "project", "organization",
                            "procedure", "media", "quote", "topic"
                        ],
                        "description": "Optional archetype classification"
                    },
                    "tags": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Optional tags for categorization"
                    },
                    "trust": {
                        "type": "integer",
                        "description": "Trust level 1-10 (default 5)"
                    }
                },
                "required": ["action", "title", "body"],
                "additionalProperties": false
            }),
        }
    }

    #[tracing::instrument(skip_all, fields(tool = "note_write"))]
    async fn execute(&self, params: Value, ctx: &ToolContext) -> Result<String, ToolError> {
        let action = params
            .get("action")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ToolError::InvalidParams("missing required parameter 'action'".into())
            })?;
        let title = params
            .get("title")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::InvalidParams("missing required parameter 'title'".into()))?;
        let body = params
            .get("body")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::InvalidParams("missing required parameter 'body'".into()))?;
        let archetype = params.get("archetype").and_then(Value::as_str);
        let tags: Vec<String> = params
            .get("tags")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_str)
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default();
        let trust = params.get("trust").and_then(Value::as_i64).unwrap_or(5);

        match action {
            "create" => {
                self.create_note(ctx, title, body, archetype, &tags, trust)
                    .await
            }
            "update" => {
                self.update_note(ctx, title, body, archetype, &tags, trust)
                    .await
            }
            _ => Err(ToolError::InvalidParams(format!(
                "unknown action '{action}', expected 'create' or 'update'"
            ))),
        }
    }
}

impl NoteWrite {
    async fn create_note(
        &self,
        ctx: &ToolContext,
        title: &str,
        body: &str,
        archetype: Option<&str>,
        tags: &[String],
        trust: i64,
    ) -> Result<String, ToolError> {
        let front = NoteFrontMatter {
            title: title.to_string(),
            archetype: archetype
                .and_then(|a| serde_json::from_value(Value::String(a.to_string())).ok()),
            tags: tags.to_vec(),
            trust,
        };

        let path = knowledge::write_note(&ctx.workspace, &front, body)
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        let note_id = db::knowledge::create_note_full(&ctx.db, title, body, archetype, tags, trust)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        let wiki_links = extract_wiki_links(body);
        let result = reconcile_edges(&ctx.db, &note_id, title, &wiki_links)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        Ok(format!(
            "Created note '{}' at {}\n\
             DB record: {}\n\
             Edges: {} created, {} stubs created",
            title,
            path.display(),
            note_id,
            result.created,
            result.stubs_created,
        ))
    }

    async fn update_note(
        &self,
        ctx: &ToolContext,
        title: &str,
        body: &str,
        archetype: Option<&str>,
        tags: &[String],
        trust: i64,
    ) -> Result<String, ToolError> {
        let existing = db::knowledge::find_note_by_title(&ctx.db, title)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?
            .ok_or_else(|| ToolError::ExecutionFailed(format!("note '{title}' not found")))?;

        db::knowledge::update_note(&ctx.db, &existing.id, body, archetype, tags, trust)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        let front = NoteFrontMatter {
            title: title.to_string(),
            archetype: archetype
                .and_then(|a| serde_json::from_value(Value::String(a.to_string())).ok()),
            tags: tags.to_vec(),
            trust,
        };
        let path = knowledge::write_note(&ctx.workspace, &front, body)
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        let wiki_links = extract_wiki_links(body);
        let result = reconcile_edges(&ctx.db, &existing.id, title, &wiki_links)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        Ok(format!(
            "Updated note '{}' at {}\n\
             Edges: {} created, {} deleted, {} stubs created",
            title,
            path.display(),
            result.created,
            result.deleted,
            result.stubs_created,
        ))
    }
}
