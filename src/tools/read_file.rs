use async_trait::async_trait;
use serde_json::{Value, json};

use crate::providers::ToolDefinition;

use super::context::{ToolContext, resolve_path};
use super::error::ToolError;
use super::manager::Tool;

pub struct ReadFile;

const DEFAULT_LIMIT: usize = 2000;

#[async_trait]
impl Tool for ReadFile {
    fn name(&self) -> &str {
        "read_file"
    }

    fn schema(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Read a file from the workspace with line numbers. \
                Supports pagination via offset and limit."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path (relative to workspace or absolute)"
                    },
                    "offset": {
                        "type": "integer",
                        "description": "Starting line number (1-based, default: 1)"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of lines to return (default: 2000)"
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

        let offset = params
            .get("offset")
            .and_then(Value::as_u64)
            .map(|v| v.max(1) as usize)
            .unwrap_or(1);

        let limit = params
            .get("limit")
            .and_then(Value::as_u64)
            .map(|v| v as usize)
            .unwrap_or(DEFAULT_LIMIT);

        let path = resolve_path(raw_path, &ctx.cwd, &ctx.workspace)?;

        let content = tokio::fs::read_to_string(&path).await.map_err(|e| {
            ToolError::ExecutionFailed(format!("failed to read '{}': {e}", path.display()))
        })?;

        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();

        if total_lines == 0 {
            return Ok(format!("File: {raw_path}\n(empty file)"));
        }

        // offset is 1-based
        let start_idx = (offset - 1).min(total_lines);
        let end_idx = (start_idx + limit).min(total_lines);
        let selected = &lines[start_idx..end_idx];

        let max_line_num = start_idx + selected.len();
        let width = max_line_num.to_string().len().max(3);

        let mut result = String::new();
        result.push_str(&format!(
            "File: {raw_path} (lines {}-{} of {total_lines})\n",
            start_idx + 1,
            end_idx,
        ));

        for (i, line) in selected.iter().enumerate() {
            let line_num = start_idx + i + 1;
            result.push_str(&format!("{line_num:>width$} | {line}\n"));
        }

        if end_idx < total_lines {
            result.push_str(&format!(
                "\n({} more lines not shown — use offset={} to continue)",
                total_lines - end_idx,
                end_idx + 1,
            ));
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
            db: surrealdb::Surreal::init(),
            config: crate::config::test_config(workspace),
            session_id: "test".to_string(),
            task_runner: None,
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
    async fn read_file_pagination() {
        let workspace = TempDir::new().unwrap();
        let file = workspace.path().join("big.txt");
        let content: String = (1..=100)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(&file, &content).unwrap();

        let ctx = test_ctx_in(workspace.path());
        let result = ReadFile
            .execute(json!({"path": "big.txt", "offset": 10, "limit": 5}), &ctx)
            .await
            .unwrap();

        assert!(result.contains("10 | line 10"));
        assert!(result.contains("14 | line 14"));
        assert!(result.contains("more lines not shown"));
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
