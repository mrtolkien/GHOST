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

        // Append extra-files block for skill.md files
        if raw_path.ends_with("skill.md")
            && path.components().any(|c| c.as_os_str() == "skills")
            && let Some(skill_dir) = path.parent()
        {
            let extras = crate::skills::collect_extras(skill_dir);
            if !extras.is_empty() {
                result.push_str("\n<extra-files>\n");
                for extra in &extras {
                    result.push_str(&format!("  <file path=\"{}\" />\n", extra.display()));
                }
                result.push_str("</extra-files>\n");
            }
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
            event_tx: None,
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
    async fn read_skill_md_appends_extra_files() {
        let workspace = TempDir::new().unwrap();
        let skill_dir = workspace.path().join("skills").join("test-skill");
        std::fs::create_dir_all(skill_dir.join("scripts")).unwrap();

        std::fs::write(
            skill_dir.join("skill.md"),
            "---\nname: test-skill\ndescription: Test.\n---\n\n# Test Skill\n",
        )
        .unwrap();
        std::fs::write(skill_dir.join("reference.md"), "ref content").unwrap();
        std::fs::write(skill_dir.join("scripts/run.py"), "print()").unwrap();

        let ctx = test_ctx_in(workspace.path());
        let result = ReadFile
            .execute(json!({"path": "skills/test-skill/skill.md"}), &ctx)
            .await
            .unwrap();

        assert!(result.contains("<extra-files>"));
        assert!(result.contains("./reference.md"));
        assert!(result.contains("./scripts/run.py"));
        assert!(result.contains("</extra-files>"));
    }

    #[tokio::test]
    async fn read_skill_md_no_extras_no_block() {
        let workspace = TempDir::new().unwrap();
        let skill_dir = workspace.path().join("skills").join("bare-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();

        std::fs::write(
            skill_dir.join("skill.md"),
            "---\nname: bare-skill\ndescription: Bare.\n---\n\n# Bare\n",
        )
        .unwrap();

        let ctx = test_ctx_in(workspace.path());
        let result = ReadFile
            .execute(json!({"path": "skills/bare-skill/skill.md"}), &ctx)
            .await
            .unwrap();

        assert!(!result.contains("<extra-files>"));
    }

    #[tokio::test]
    async fn read_skill_md_excludes_agent_dirs() {
        let workspace = TempDir::new().unwrap();
        let skill_dir = workspace.path().join("skills").join("with-agent");
        std::fs::create_dir_all(skill_dir.join("my-agent")).unwrap();

        std::fs::write(
            skill_dir.join("skill.md"),
            "---\nname: with-agent\ndescription: Has agent.\n---\n\n# Agent Skill\n",
        )
        .unwrap();
        std::fs::write(skill_dir.join("my-agent/agent.lua"), "return {}").unwrap();
        std::fs::write(skill_dir.join("my-agent/prompt.md"), "prompt").unwrap();

        let ctx = test_ctx_in(workspace.path());
        let result = ReadFile
            .execute(json!({"path": "skills/with-agent/skill.md"}), &ctx)
            .await
            .unwrap();

        assert!(!result.contains("<extra-files>"));
        assert!(!result.contains("agent.lua"));
        assert!(!result.contains("prompt.md"));
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
