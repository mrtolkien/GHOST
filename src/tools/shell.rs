use async_trait::async_trait;
use serde_json::{Value, json};

use crate::providers::ToolDefinition;

use super::context::ToolContext;
use super::error::ToolError;
use super::manager::Tool;

pub struct RunShellCommand;

const DEFAULT_TIMEOUT_MS: u64 = 30_000;
const MAX_TIMEOUT_MS: u64 = 300_000;

#[async_trait]
impl Tool for RunShellCommand {
    fn name(&self) -> &str {
        "run_shell_command"
    }

    fn schema(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Run a shell command in the workspace. Returns stdout, \
                stderr, and exit code. Non-zero exit codes are reported (not \
                errors) so you can see the output."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The shell command to execute"
                    },
                    "timeout_ms": {
                        "type": "integer",
                        "description": "Timeout in milliseconds (default 30000, max 300000)"
                    },
                    "directory": {
                        "type": "string",
                        "description": "Working directory (relative to workspace, default: workspace root)"
                    }
                },
                "required": ["command"],
                "additionalProperties": false
            }),
        }
    }

    #[tracing::instrument(skip_all, fields(tool = "run_shell_command"))]
    async fn execute(&self, params: Value, ctx: &ToolContext) -> Result<String, ToolError> {
        let command = params
            .get("command")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ToolError::InvalidParams("missing required parameter 'command'".to_string())
            })?;

        let timeout_ms = params
            .get("timeout_ms")
            .and_then(Value::as_u64)
            .unwrap_or(DEFAULT_TIMEOUT_MS)
            .min(MAX_TIMEOUT_MS);

        let work_dir = if let Some(dir) = params.get("directory").and_then(Value::as_str) {
            super::context::resolve_path(dir, &ctx.cwd, &ctx.workspace)?
        } else {
            ctx.cwd.clone()
        };

        let timeout = std::time::Duration::from_millis(timeout_ms);

        let child = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(command)
            .current_dir(&work_dir)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to spawn shell: {e}")))?;

        let output = match tokio::time::timeout(timeout, child.wait_with_output()).await {
            Ok(Ok(output)) => output,
            Ok(Err(e)) => {
                return Err(ToolError::ExecutionFailed(format!("command failed: {e}")));
            }
            Err(_) => {
                return Ok(format!(
                    "Command timed out after {timeout_ms}ms.\n\
                     Hint: increase timeout_ms or break the command into smaller steps."
                ));
            }
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let exit_code = output.status.code().unwrap_or(-1);

        let mut result = String::new();
        result.push_str(&format!("Exit code: {exit_code}\n"));

        if !stdout.is_empty() {
            result.push_str(&format!("\n--- stdout ---\n{stdout}"));
        }
        if !stderr.is_empty() {
            result.push_str(&format!("\n--- stderr ---\n{stderr}"));
        }
        if stdout.is_empty() && stderr.is_empty() {
            result.push_str("\n(no output)");
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_ctx() -> (ToolContext, TempDir) {
        let workspace = TempDir::new().unwrap();
        let ctx = ToolContext {
            workspace: workspace.path().to_path_buf(),
            cwd: workspace.path().to_path_buf(),
            db: sqlx::SqlitePool::connect_lazy("sqlite::memory:").unwrap(),
            config: crate::config::test_config(workspace.path()),
            session_id: "test".to_string(),
            task_runner: None,
        };
        (ctx, workspace)
    }

    #[tokio::test]
    async fn echo_command() {
        let (ctx, _ws) = test_ctx();
        let result = RunShellCommand
            .execute(json!({"command": "echo hello"}), &ctx)
            .await;
        let output = result.unwrap();
        assert!(output.contains("Exit code: 0"));
        assert!(output.contains("hello"));
    }

    #[tokio::test]
    async fn exit_code_nonzero() {
        let (ctx, _ws) = test_ctx();
        let result = RunShellCommand
            .execute(json!({"command": "exit 42"}), &ctx)
            .await;
        let output = result.unwrap();
        assert!(output.contains("Exit code: 42"));
    }

    #[tokio::test]
    async fn timeout() {
        let (ctx, _ws) = test_ctx();
        let result = RunShellCommand
            .execute(json!({"command": "sleep 10", "timeout_ms": 100}), &ctx)
            .await;
        let output = result.unwrap();
        assert!(output.contains("timed out"));
    }

    #[tokio::test]
    async fn missing_command_param() {
        let (ctx, _ws) = test_ctx();
        let result = RunShellCommand.execute(json!({}), &ctx).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ToolError::InvalidParams(_)));
    }

    #[tokio::test]
    async fn stderr_captured() {
        let (ctx, _ws) = test_ctx();
        let result = RunShellCommand
            .execute(json!({"command": "echo oops >&2"}), &ctx)
            .await;
        let output = result.unwrap();
        assert!(output.contains("stderr"));
        assert!(output.contains("oops"));
    }
}
