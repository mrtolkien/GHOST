use async_trait::async_trait;
use serde_json::{Value, json};

use crate::providers::ToolDefinition;

use super::context::{ToolContext, resolve_path};
use super::error::ToolError;
use super::manager::Tool;

pub struct ReadFile;

#[async_trait]
impl Tool for ReadFile {
    fn name(&self) -> &str {
        "read_file"
    }

    fn schema(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Read a file from the workspace with line numbers.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path (relative to workspace or absolute)"
                    }
                },
                "required": ["path"],
                "additionalProperties": false
            }),
        }
    }

    #[tracing::instrument(skip_all, fields(tool = "read_file"))]
    async fn execute(&self, params: Value, ctx: &ToolContext) -> Result<String, ToolError> {
        let raw_path = params.get("path").and_then(Value::as_str).ok_or_else(|| {
            ToolError::InvalidParams("missing required parameter 'path'".to_string())
        })?;

        let path = resolve_path(raw_path, &ctx.cwd, &ctx.workspace)?;

        let content = tokio::fs::read_to_string(&path).await.map_err(|e| {
            ToolError::ExecutionFailed(format!("failed to read '{}': {e}", path.display()))
        })?;

        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();

        if total_lines == 0 {
            return Ok(format!("File: {raw_path}\n(empty file)"));
        }

        let width = total_lines.to_string().len().max(3);

        let mut result = String::new();
        result.push_str(&format!("File: {raw_path} ({total_lines} lines)\n"));

        for (i, line) in lines.iter().enumerate() {
            let line_num = i + 1;
            result.push_str(&format!("{line_num:>width$} | {line}\n"));
        }

        Ok(result)
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
            completion_tx: None,
            channel_id: None,
        }
    }

    #[tokio::test]
    async fn read_file_with_line_numbers() {
        let workspace = TempDir::new().unwrap();
        let file = workspace.path().join("test.txt");
        std::fs::write(&file, "line one\nline two\nline three\n").unwrap();

        let ctx = test_ctx_in(workspace.path());
        let result = ReadFile
            .execute(json!({"path": "test.txt"}), &ctx)
            .await
            .unwrap();

        assert!(result.contains("test.txt"));
        assert!(result.contains("  1 | line one"));
        assert!(result.contains("  2 | line two"));
        assert!(result.contains("  3 | line three"));
    }

    #[tokio::test]
    async fn read_file_not_found() {
        let workspace = TempDir::new().unwrap();
        let ctx = test_ctx_in(workspace.path());
        let result = ReadFile
            .execute(json!({"path": "nonexistent.txt"}), &ctx)
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn read_empty_file() {
        let workspace = TempDir::new().unwrap();
        let file = workspace.path().join("empty.txt");
        std::fs::write(&file, "").unwrap();

        let ctx = test_ctx_in(workspace.path());
        let result = ReadFile
            .execute(json!({"path": "empty.txt"}), &ctx)
            .await
            .unwrap();

        assert!(result.contains("empty file"));
    }
}
