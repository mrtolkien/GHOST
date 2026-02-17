use async_trait::async_trait;
use serde_json::{Value, json};

use crate::db;
use crate::knowledge;
use crate::providers::ToolDefinition;

use super::context::ToolContext;
use super::error::ToolError;
use super::manager::Tool;

pub struct ReferenceManage;

#[async_trait]
impl Tool for ReferenceManage {
    fn name(&self) -> &str {
        "reference_manage"
    }

    fn schema(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Manage knowledge references. Move web-cache files into \
                the knowledge/references directory (preserving citation edges) \
                or delete references."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["move", "delete"],
                        "description": "Action to perform"
                    },
                    "cache_file": {
                        "type": "string",
                        "description": "Path to the .web-cache file to move (required for 'move')"
                    },
                    "target_topic": {
                        "type": "string",
                        "description": "Topic directory for the moved reference (required for 'move')"
                    },
                    "target_filename": {
                        "type": "string",
                        "description": "Filename for the moved reference (required for 'move')"
                    },
                    "reference_path": {
                        "type": "string",
                        "description": "Path of the reference to delete (required for 'delete')"
                    }
                },
                "required": ["action"],
                "additionalProperties": false
            }),
        }
    }

    #[tracing::instrument(skip_all, fields(tool = "reference_manage"))]
    async fn execute(&self, params: Value, ctx: &ToolContext) -> Result<String, ToolError> {
        let action = params
            .get("action")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ToolError::InvalidParams("missing required parameter 'action'".into())
            })?;

        match action {
            "move" => self.move_reference(ctx, &params).await,
            "delete" => self.delete_reference(ctx, &params).await,
            _ => Err(ToolError::InvalidParams(format!(
                "unknown action '{action}', expected 'move' or 'delete'"
            ))),
        }
    }
}

impl ReferenceManage {
    async fn move_reference(&self, ctx: &ToolContext, params: &Value) -> Result<String, ToolError> {
        let cache_file = params
            .get("cache_file")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ToolError::InvalidParams("missing required parameter 'cache_file' for move".into())
            })?;
        let target_topic = params
            .get("target_topic")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ToolError::InvalidParams(
                    "missing required parameter 'target_topic' for move".into(),
                )
            })?;
        let target_filename = params
            .get("target_filename")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ToolError::InvalidParams(
                    "missing required parameter 'target_filename' for move".into(),
                )
            })?;

        let cache_path = ctx.workspace.join(cache_file);
        if !cache_path.exists() {
            return Err(ToolError::ExecutionFailed(format!(
                "cache file not found: {}",
                cache_path.display()
            )));
        }

        let content = tokio::fs::read_to_string(&cache_path)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to read cache file: {e}")))?;

        let target_path = knowledge::reference_path(&ctx.workspace, target_topic, target_filename);
        if let Some(parent) = target_path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                ToolError::ExecutionFailed(format!("failed to create directory: {e}"))
            })?;
        }

        tokio::fs::write(&target_path, &content)
            .await
            .map_err(|e| {
                ToolError::ExecutionFailed(format!("failed to write reference file: {e}"))
            })?;

        let relative_target = format!("knowledge/references/{target_topic}/{target_filename}");

        // Update existing DB record or create a new one
        if let Some(ref_record) = db::knowledge::find_reference_by_path(&ctx.db, cache_file)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?
        {
            db::knowledge::update_reference_path(
                &ctx.db,
                &ref_record.id,
                &relative_target,
                target_topic,
            )
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        }

        // Delete the cache file
        tokio::fs::remove_file(&cache_path)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to remove cache file: {e}")))?;

        Ok(format!(
            "Moved {} -> {}\nCited edges preserved.",
            cache_file, relative_target
        ))
    }

    async fn delete_reference(
        &self,
        ctx: &ToolContext,
        params: &Value,
    ) -> Result<String, ToolError> {
        let ref_path = params
            .get("reference_path")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ToolError::InvalidParams(
                    "missing required parameter 'reference_path' for delete".into(),
                )
            })?;

        if let Some(ref_record) = db::knowledge::find_reference_by_path(&ctx.db, ref_path)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?
        {
            db::knowledge::delete_reference(&ctx.db, &ref_record.id)
                .await
                .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        }

        let file_path = ctx.workspace.join(ref_path);
        if file_path.exists() {
            tokio::fs::remove_file(&file_path)
                .await
                .map_err(|e| ToolError::ExecutionFailed(format!("failed to remove file: {e}")))?;
        }

        Ok(format!("Deleted reference: {ref_path}"))
    }
}
