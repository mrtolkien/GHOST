use super::output::ToolOutput;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::db;
use crate::providers::ToolDefinition;

use super::context::ToolContext;
use super::error::ToolError;
use super::manager::Tool;

pub struct RunShellCommand;

use crate::constants::{DEFAULT_SHELL_TIMEOUT_MS, MAX_SHELL_OUTPUT_CHARS};

static BACKGROUND_SHELL_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Number of currently running background shell commands.
pub fn background_shell_count() -> usize {
    BACKGROUND_SHELL_COUNT.load(Ordering::Relaxed)
}

/// Build the workspace's nix shell environment and write its `bin/` path
/// to `$WORKSPACE/.shell-bin`. Called at daemon boot and by `ghost shell rebuild`.
pub async fn rebuild_shell_env(workspace: &std::path::Path) -> Result<(), String> {
    let shell_dir = workspace.join("shell");
    if !shell_dir.join("flake.nix").exists() {
        return Ok(());
    }

    let shell_dir_str = shell_dir.display().to_string();
    tracing::info!(shell_dir = %shell_dir_str, "building nix shell environment");
    let output = tokio::process::Command::new("nix")
        .args(["build", &shell_dir_str, "--no-link", "--print-out-paths"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .await
        .map_err(|e| format!("failed to run nix build: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("nix build failed: {stderr}"));
    }

    let store_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !store_path.is_empty() {
        let bin_path = format!("{store_path}/bin");
        std::fs::write(workspace.join(crate::nix::SHELL_BIN_FILE), &bin_path)
            .map_err(|e| format!("failed to write {}: {e}", crate::nix::SHELL_BIN_FILE))?;
        tracing::info!(bin_path, "nix shell environment built");
    }

    Ok(())
}

/// Build a `Command` that runs `sh -c <cmd>`, prepending the home-manager
/// profile PATH if available. Sets `GHOST_CHANNEL_ID` / `GHOST_SESSION_ID`
/// so child processes can auto-detect the calling context.
fn shell_command(
    command: &str,
    workspace: &std::path::Path,
    channel_id: Option<&str>,
    session_id: Option<&str>,
) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new("/bin/sh");
    cmd.args(["-c", command]);

    cmd.env("PATH", crate::nix::nix_path(workspace));

    if let Some(id) = channel_id {
        cmd.env("GHOST_CHANNEL_ID", id);
    }
    if let Some(id) = session_id {
        cmd.env("GHOST_SESSION_ID", id);
    }
    cmd
}

#[async_trait]
impl Tool for RunShellCommand {
    fn name(&self) -> &str {
        "shell"
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

    #[tracing::instrument(skip_all, fields(tool = "shell"))]
    async fn execute(&self, params: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
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

            BACKGROUND_SHELL_COUNT.fetch_add(1, Ordering::Relaxed);
            tokio::spawn(async move {
                let child = shell_command(
                    &command_owned,
                    &workspace_owned,
                    channel_id.as_deref(),
                    Some(&session_id),
                )
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
                if truncated.len() > MAX_SHELL_OUTPUT_CHARS {
                    let end = truncated.floor_char_boundary(MAX_SHELL_OUTPUT_CHARS);
                    truncated.truncate(end);
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
                        notify_only: false,
                    });
                }

                BACKGROUND_SHELL_COUNT.fetch_sub(1, Ordering::Relaxed);
            });

            return Ok(ToolOutput::text(
                "Command started in background. You'll see the result as a \
                 system message when it completes."
                    .to_string(),
            ));
        }

        // Foreground path
        let timeout_ms = params
            .get("timeout_ms")
            .and_then(Value::as_u64)
            .unwrap_or(DEFAULT_SHELL_TIMEOUT_MS);

        let timeout = std::time::Duration::from_millis(timeout_ms);

        let child = shell_command(
            command,
            &ctx.workspace,
            ctx.channel_id.as_deref(),
            Some(&ctx.session_id),
        )
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
                return Ok(ToolOutput::text(format!(
                    "Command timed out after {timeout_ms}ms.\n\
                     Hint: increase timeout_ms or break the command into smaller steps."
                )));
            }
        };

        Ok(ToolOutput::text(format_output(&output)))
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
            config: std::sync::Arc::new(crate::config::test_config(workspace.path())),
            session_id: "test".to_string(),
            agent_runner: None,
            event_tx: None,
            channel_id: None,
            confirmation_tx: None,
            browser_manager: std::sync::Arc::new(tokio::sync::Mutex::new(
                crate::web::browser::BrowserManager::new(vec![]),
            )),
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
        assert!(output.text.contains("Exit code: 0"));
        assert!(output.text.contains("hello"));
    }

    #[tokio::test]
    async fn exit_code_nonzero() {
        let (ctx, _ws) = test_ctx();
        let result = RunShellCommand
            .execute(json!({"command": "exit 42"}), &ctx)
            .await;
        let output = result.unwrap();
        assert!(output.text.contains("Exit code: 42"));
    }

    #[tokio::test]
    async fn timeout() {
        let (ctx, _ws) = test_ctx();
        let result = RunShellCommand
            .execute(json!({"command": "sleep 10", "timeout_ms": 100}), &ctx)
            .await;
        let output = result.unwrap();
        assert!(output.text.contains("timed out"));
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
        assert!(output.text.contains("stderr"));
        assert!(output.text.contains("oops"));
    }

    #[tokio::test]
    async fn background_returns_immediately() {
        let (ctx, _ws) = test_ctx();
        let result = RunShellCommand
            .execute(json!({"command": "echo bg-test", "background": true}), &ctx)
            .await;
        let output = result.unwrap();
        assert!(output.text.contains("background"));
    }
}
