use async_trait::async_trait;
use serde_json::{Value, json};

use crate::providers::ToolDefinition;

use super::context::{ToolContext, resolve_path};
use super::error::ToolError;
use super::manager::Tool;

pub struct WriteFile;

#[async_trait]
impl Tool for WriteFile {
    fn name(&self) -> &str {
        "write_file"
    }

    fn schema(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Create or overwrite a file in the workspace. Parent \
                directories are created automatically."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path (relative to workspace or absolute)"
                    },
                    "content": {
                        "type": "string",
                        "description": "Content to write to the file"
                    }
                },
                "required": ["path", "content"],
                "additionalProperties": false
            }),
        }
    }

    #[tracing::instrument(skip_all, fields(tool = "write_file"))]
    async fn execute(&self, params: Value, ctx: &ToolContext) -> Result<String, ToolError> {
        let raw_path = params.get("path").and_then(Value::as_str).ok_or_else(|| {
            ToolError::InvalidParams("missing required parameter 'path'".to_string())
        })?;

        let content = params
            .get("content")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ToolError::InvalidParams("missing required parameter 'content'".to_string())
            })?;

        let path = resolve_path(raw_path, &ctx.cwd, &ctx.workspace)?;

        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                ToolError::ExecutionFailed(format!(
                    "failed to create parent directories for '{}': {e}",
                    path.display()
                ))
            })?;
        }

        let existed = path.exists();
        let bytes = content.len();

        tokio::fs::write(&path, content).await.map_err(|e| {
            ToolError::ExecutionFailed(format!("failed to write '{}': {e}", path.display()))
        })?;

        let action = if existed { "Updated" } else { "Created" };
        let lines = content.lines().count();
        Ok(format!(
            "{action} {raw_path} ({lines} lines, {bytes} bytes)"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_ctx_in(workspace: &std::path::Path) -> ToolContext {
        ToolContext {
            workspace: workspace.to_path_buf(),
            cwd: workspace.to_path_buf(),
            db: sqlx::SqlitePool::connect_lazy("sqlite::memory:").unwrap(),
            config: crate::config::test_config(workspace),
            session_id: "test".to_string(),
            agent_runner: None,
            event_tx: None,
            channel_id: None,
        }
    }

    #[tokio::test]
    async fn create_new_file() {
        let workspace = TempDir::new().unwrap();
        let ctx = test_ctx_in(workspace.path());

        let result = WriteFile
            .execute(json!({"path": "new.txt", "content": "hello world"}), &ctx)
            .await
            .unwrap();

        assert!(result.contains("Created"));
        let content = std::fs::read_to_string(workspace.path().join("new.txt")).unwrap();
        assert_eq!(content, "hello world");
    }

    #[tokio::test]
    async fn create_with_parent_dirs() {
        let workspace = TempDir::new().unwrap();
        let ctx = test_ctx_in(workspace.path());

        let result = WriteFile
            .execute(
                json!({"path": "deep/nested/dir/file.txt", "content": "data"}),
                &ctx,
            )
            .await
            .unwrap();

        assert!(result.contains("Created"));
        let content =
            std::fs::read_to_string(workspace.path().join("deep/nested/dir/file.txt")).unwrap();
        assert_eq!(content, "data");
    }

    #[tokio::test]
    async fn overwrite_existing() {
        let workspace = TempDir::new().unwrap();
        let file = workspace.path().join("exists.txt");
        std::fs::write(&file, "old content").unwrap();

        let ctx = test_ctx_in(workspace.path());
        let result = WriteFile
            .execute(
                json!({"path": "exists.txt", "content": "new content"}),
                &ctx,
            )
            .await
            .unwrap();

        assert!(result.contains("Updated"));
        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(content, "new content");
    }
}
