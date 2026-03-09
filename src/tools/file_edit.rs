use super::output::ToolOutput;
use async_trait::async_trait;
use serde_json::{Value, json};

use crate::providers::ToolDefinition;

use super::context::{ToolContext, resolve_path};
use super::error::ToolError;
use super::manager::Tool;

pub struct FileEdit;

#[async_trait]
impl Tool for FileEdit {
    fn name(&self) -> &str {
        "file_edit"
    }

    fn schema(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Replace a unique string in a file. The old_string must \
                appear exactly once in the file. Use this for precise edits \
                instead of rewriting entire files."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path (relative to workspace or absolute)"
                    },
                    "old_string": {
                        "type": "string",
                        "description": "The exact string to find (must appear exactly once)"
                    },
                    "new_string": {
                        "type": "string",
                        "description": "The replacement string"
                    }
                },
                "required": ["path", "old_string", "new_string"],
                "additionalProperties": false
            }),
        }
    }

    #[tracing::instrument(skip_all, fields(tool = "file_edit"))]
    async fn execute(&self, params: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let raw_path = params.get("path").and_then(Value::as_str).ok_or_else(|| {
            ToolError::InvalidParams("missing required parameter 'path'".to_string())
        })?;

        let old_string = params
            .get("old_string")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ToolError::InvalidParams("missing required parameter 'old_string'".to_string())
            })?;

        let new_string = params
            .get("new_string")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ToolError::InvalidParams("missing required parameter 'new_string'".to_string())
            })?;

        let path = resolve_path(raw_path, &ctx.cwd, &ctx.workspace)?;

        let content = tokio::fs::read_to_string(&path).await.map_err(|e| {
            ToolError::ExecutionFailed(format!("failed to read '{}': {e}", path.display()))
        })?;

        let count = content.matches(old_string).count();

        if count == 0 {
            return Err(ToolError::ExecutionFailed(format!(
                "old_string not found in '{raw_path}'. Make sure it matches \
                 exactly (including whitespace and indentation)."
            )));
        }

        if count > 1 {
            return Err(ToolError::ExecutionFailed(format!(
                "old_string found {count} times in '{raw_path}'. It must be \
                 unique — include more surrounding context to disambiguate."
            )));
        }

        let new_content = content.replacen(old_string, new_string, 1);
        tokio::fs::write(&path, &new_content).await.map_err(|e| {
            ToolError::ExecutionFailed(format!("failed to write '{}': {e}", path.display()))
        })?;

        // Show context around the edit
        let edit_line = content
            .lines()
            .enumerate()
            .find(|(_, line)| line.contains(old_string))
            .map(|(i, _)| i + 1)
            .unwrap_or(0);

        Ok(ToolOutput::text(format!(
            "Edited {raw_path} at line {edit_line}: replaced 1 occurrence."
        )))
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
    async fn single_match_replaced() {
        let workspace = TempDir::new().unwrap();
        let file = workspace.path().join("code.rs");
        std::fs::write(&file, "fn hello() {\n    println!(\"hi\");\n}\n").unwrap();

        let ctx = test_ctx_in(workspace.path());
        let result = FileEdit
            .execute(
                json!({
                    "path": "code.rs",
                    "old_string": "println!(\"hi\")",
                    "new_string": "println!(\"hello world\")"
                }),
                &ctx,
            )
            .await
            .unwrap();

        assert!(result.text.contains("Edited"));
        let content = std::fs::read_to_string(&file).unwrap();
        assert!(content.contains("hello world"));
        assert!(!content.contains("\"hi\""));
    }

    #[tokio::test]
    async fn zero_matches_error() {
        let workspace = TempDir::new().unwrap();
        let file = workspace.path().join("test.txt");
        std::fs::write(&file, "some content").unwrap();

        let ctx = test_ctx_in(workspace.path());
        let result = FileEdit
            .execute(
                json!({
                    "path": "test.txt",
                    "old_string": "nonexistent string",
                    "new_string": "replacement"
                }),
                &ctx,
            )
            .await;

        assert!(result.is_err());
        let err = format!("{}", result.unwrap_err());
        assert!(err.contains("not found"));
    }

    #[tokio::test]
    async fn multiple_matches_error() {
        let workspace = TempDir::new().unwrap();
        let file = workspace.path().join("test.txt");
        std::fs::write(&file, "hello hello hello").unwrap();

        let ctx = test_ctx_in(workspace.path());
        let result = FileEdit
            .execute(
                json!({
                    "path": "test.txt",
                    "old_string": "hello",
                    "new_string": "world"
                }),
                &ctx,
            )
            .await;

        assert!(result.is_err());
        let err = format!("{}", result.unwrap_err());
        assert!(err.contains("3 times"));
    }
}
