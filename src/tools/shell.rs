use async_trait::async_trait;
use serde_json::{Value, json};

use crate::db;
use crate::providers::ToolDefinition;

use super::context::ToolContext;
use super::error::ToolError;
use super::manager::Tool;

pub struct RunShellCommand;

const DEFAULT_TIMEOUT_MS: u64 = 30_000;
const MAX_OUTPUT_CHARS: usize = 50_000;

/// Build a `Command` that runs `sh -c <cmd>` inside `nix develop` if the
/// workspace has a `shell/flake.nix`, otherwise runs directly.
/// Sets `GHOST_CHANNEL_ID` if available so child processes (e.g. `ghost hack
/// start`) can auto-detect the calling channel.
fn shell_command(
    command: &str,
    workspace: &std::path::Path,
    channel_id: Option<&str>,
) -> tokio::process::Command {
    let shell_dir = workspace.join("shell");
    let mut cmd = if shell_dir.join("flake.nix").exists() {
        let mut cmd = tokio::process::Command::new("nix");
        cmd.args([
            "develop",
            "--keep",
            "PATH",
            shell_dir.to_str().unwrap_or("."),
            "--command",
            "sh",
            "-c",
            command,
        ]);
        cmd
    } else {
        let mut cmd = tokio::process::Command::new("sh");
        cmd.args(["-c", command]);
        cmd
    };
    if let Some(id) = channel_id {
        cmd.env("GHOST_CHANNEL_ID", id);
    }
    cmd
}

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
                errors) so you can see the output. Use background=true for \
                long-running commands — the result arrives as a system message."
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
                        "description": "Timeout in milliseconds (default 30000). Ignored when background=true."
                    },
                    "directory": {
                        "type": "string",
                        "description": "Working directory (relative to workspace, default: workspace root)"
                    },
                    "background": {
                        "type": "boolean",
                        "description": "Run in background with no timeout. Result is delivered as a system message when complete."
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

        let background = params
            .get("background")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        let work_dir = if let Some(dir) = params.get("directory").and_then(Value::as_str) {
            super::context::resolve_path(dir, &ctx.cwd, &ctx.workspace)?
        } else {
            ctx.cwd.clone()
        };

        if background {
            let db = ctx.db.clone();
            let session_id = ctx.session_id.clone();
            let command_owned = command.to_string();
            let work_dir_owned = work_dir.clone();
            let event_tx = ctx.event_tx.clone();
            let channel_id = ctx.channel_id.clone();

            let workspace_owned = ctx.workspace.clone();

            tokio::spawn(async move {
                let child = shell_command(&command_owned, &workspace_owned, channel_id.as_deref())
                    .current_dir(&work_dir_owned)
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn();

                let output_text = match child {
                    Ok(child) => match child.wait_with_output().await {
                        Ok(output) => format_output(&output),
                        Err(e) => format!("Command failed: {e}"),
                    },
                    Err(e) => format!("Failed to spawn shell: {e}"),
                };

                let mut truncated = output_text;
                if truncated.len() > MAX_OUTPUT_CHARS {
                    truncated.truncate(MAX_OUTPUT_CHARS);
                    truncated.push_str("\n...[truncated]");
                }

                let msg = format!("[shell-command completed]\n$ {command_owned}\n\n{truncated}");

                if let Err(e) = db::sessions::create_message(&db, &session_id, "system", &msg).await
                {
                    tracing::error!(
                        error = %e,
                        session_id,
                        "failed to post background shell result"
                    );
                }

                if let Some(ref tx) = event_tx {
                    let _ = tx.send(crate::events::SessionEvent {
                        session_id: session_id.clone(),
                        system_message: msg.clone(),
                        discord: None,
                    });
                }
            });

            return Ok("Command started in background. You'll see the result as a \
                 system message when it completes."
                .to_string());
        }

        // Foreground path
        let timeout_ms = params
            .get("timeout_ms")
            .and_then(Value::as_u64)
            .unwrap_or(DEFAULT_TIMEOUT_MS);

        let timeout = std::time::Duration::from_millis(timeout_ms);

        let child = shell_command(command, &ctx.workspace, ctx.channel_id.as_deref())
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

        Ok(format_output(&output))
    }
}

fn format_output(output: &std::process::Output) -> String {
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

    result
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
            agent_runner: None,
            event_tx: None,
            channel_id: None,
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

    #[tokio::test]
    async fn background_returns_immediately() {
        let (ctx, _ws) = test_ctx();
        let result = RunShellCommand
            .execute(json!({"command": "echo bg-test", "background": true}), &ctx)
            .await;
        let output = result.unwrap();
        assert!(output.contains("background"));
    }
}
