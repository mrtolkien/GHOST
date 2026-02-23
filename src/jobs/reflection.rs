use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use surrealdb::Surreal;
use surrealdb::engine::local::Db;
use surrealdb::sql::Thing;
use tokio::sync::Mutex;

use crate::agents::TaskRunner;
use crate::config::Config;
use crate::db;
use crate::db::sessions::MessageRecord;
use crate::web::scan_web_cache;

pub struct ReflectionManager {
    db: Surreal<Db>,
    config: Config,
    task_runner: Arc<TaskRunner>,
    running: Arc<Mutex<()>>,
}

impl ReflectionManager {
    #[must_use]
    pub fn new(db: Surreal<Db>, config: Config, task_runner: Arc<TaskRunner>) -> Self {
        Self {
            db,
            config,
            task_runner,
            running: Arc::new(Mutex::new(())),
        }
    }

    /// Run reflection after a heartbeat, with delay and skip logic.
    #[tracing::instrument(skip_all, fields(session_id = %session_id))]
    pub async fn run_after_heartbeat(&self, session_id: &str, session_thing: &Thing) {
        // Delay before running
        let delay = Duration::from_secs(self.config.timing.reflection_idle_minutes * 60);
        tokio::time::sleep(delay).await;

        // Skip if no new messages since last reflection
        let state_path = self
            .config
            .workspace
            .join(".state")
            .join("reflection.last.md");
        if state_path.exists()
            && let Ok(metadata) = std::fs::metadata(&state_path)
            && let Ok(modified) = metadata.modified()
        {
            let since: DateTime<Utc> = modified.into();
            match db::sessions::count_messages_since(&self.db, session_thing, &since).await {
                Ok(0) => {
                    logfire::info!(
                        "reflection skipped: no new activity",
                        session_id = session_id.to_string(),
                    );
                    return;
                }
                Ok(_) => {}
                Err(e) => {
                    logfire::warn!(
                        "reflection: failed to check activity",
                        error = e.to_string(),
                    );
                }
            }
        }

        self.run(session_id, session_thing).await;
    }

    /// Run reflection on reboot — always runs, no skip logic.
    #[tracing::instrument(skip_all, fields(session_id = %session_id))]
    pub async fn run_on_reboot(&self, session_id: &str, session_thing: &Thing) {
        self.run(session_id, session_thing).await;
    }

    #[tracing::instrument(skip_all, fields(session_id = %session_id))]
    async fn run(&self, session_id: &str, session_thing: &Thing) {
        // Only one reflection at a time
        let _guard = match self.running.try_lock() {
            Ok(guard) => guard,
            Err(_) => {
                logfire::info!(
                    "reflection skipped: already running",
                    session_id = session_id.to_string(),
                );
                return;
            }
        };

        logfire::info!("reflection started", session_id = session_id.to_string(),);

        // Build user message from context variables
        let user_message = match self.build_user_message(session_thing).await {
            Ok(msg) => msg,
            Err(e) => {
                logfire::error!(
                    "reflection: failed to build user message",
                    error = e.to_string(),
                );
                return;
            }
        };

        match self
            .task_runner
            .run_to_completion("reflection", &user_message, Some(session_thing))
            .await
        {
            Ok(findings) => {
                // Save handoff note
                let state_dir = self.config.workspace.join(".state");
                let _ = std::fs::create_dir_all(&state_dir);
                let state_path = state_dir.join("reflection.last.md");
                if let Err(e) = std::fs::write(&state_path, &findings) {
                    logfire::warn!("reflection: failed to write state", error = e.to_string(),);
                }

                // Clear web cache on success
                if let Err(e) = clear_web_cache(&self.config.workspace) {
                    logfire::warn!(
                        "reflection: failed to clear web cache",
                        error = e.to_string(),
                    );
                }

                logfire::info!("reflection completed", session_id = session_id.to_string(),);
            }
            Err(e) => {
                logfire::error!(
                    "reflection failed",
                    session_id = session_id.to_string(),
                    error = e.to_string(),
                );
            }
        }
    }

    async fn build_user_message(&self, session_thing: &Thing) -> Result<String, db::DatabaseError> {
        let previous_handoff = load_state_file(&self.config.workspace, "reflection.last.md")
            .unwrap_or_else(|| "No previous handoff.".to_string());

        let diary_today = load_diary_today(&self.config.workspace)
            .unwrap_or_else(|| "No diary entry for today.".to_string());

        let messages = db::sessions::list_messages_by_session(&self.db, session_thing).await?;
        let transcript = filter_transcript(&messages);

        let web_cache_files = scan_web_cache(&self.config.workspace)
            .unwrap_or(None)
            .unwrap_or_else(|| "No cached files.".to_string());

        Ok(build_reflection_user_message(
            &previous_handoff,
            &diary_today,
            &transcript,
            &web_cache_files,
        ))
    }
}

/// Build the user message for the reflection agent from context variables.
#[must_use]
pub fn build_reflection_user_message(
    previous_handoff: &str,
    diary_today: &str,
    transcript: &str,
    web_cache_files: &str,
) -> String {
    format!(
        "## Previous Handoff Note\n\
         {previous_handoff}\n\
         \n\
         ## Today's Diary\n\
         {diary_today}\n\
         \n\
         ## Conversation Transcript (filtered)\n\
         Tool results are stripped — use `read_file` to retrieve content \
         saved during the conversation.\n\
         \n\
         {transcript}\n\
         \n\
         ## Web Cache Files\n\
         {web_cache_files}"
    )
}

/// Filter a transcript for reflection: preserve user/assistant text,
/// preserve tool call names+inputs, strip tool results.
pub fn filter_transcript(messages: &[MessageRecord]) -> String {
    let mut lines = Vec::new();

    for msg in messages {
        match msg.role.as_str() {
            "user" => {
                // Check for tool results — if present, this is a
                // tool-result message and we skip it
                if msg.tool_results.is_some() {
                    continue;
                }
                if !msg.content.trim().is_empty() {
                    lines.push(format!("[user] {}", msg.content));
                }
            }
            "assistant" => {
                if !msg.content.trim().is_empty() {
                    lines.push(format!("[assistant] {}", msg.content));
                }
                // Include tool call names + brief summary
                if let Some(ref calls) = msg.tool_calls {
                    for call in calls {
                        let name = call
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown");
                        let input = call
                            .get("input")
                            .map(|v| {
                                let s = v.to_string();
                                if s.len() > 200 {
                                    format!("{}...", &s[..200])
                                } else {
                                    s
                                }
                            })
                            .unwrap_or_default();
                        lines.push(format!("[tool_call] {name}({input})"));
                    }
                }
            }
            _ => {}
        }
    }

    lines.join("\n")
}

fn load_state_file(workspace: &Path, filename: &str) -> Option<String> {
    let path = workspace.join(".state").join(filename);
    match std::fs::read_to_string(&path) {
        Ok(content) if !content.trim().is_empty() => Some(content),
        _ => None,
    }
}

fn load_diary_today(workspace: &Path) -> Option<String> {
    let today = Utc::now().format("%Y-%m-%d").to_string();
    let path = workspace.join("diary").join(format!("{today}.md"));
    match std::fs::read_to_string(&path) {
        Ok(content) if !content.trim().is_empty() => Some(content),
        _ => None,
    }
}

/// Clear all files in the `.web-cache/` directory.
pub fn clear_web_cache(workspace: &Path) -> Result<(), std::io::Error> {
    let cache_dir = workspace.join(".web-cache");
    if !cache_dir.exists() {
        return Ok(());
    }

    for entry in std::fs::read_dir(&cache_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            std::fs::remove_file(&path)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use surrealdb::sql::Datetime;
    use tempfile::TempDir;

    fn make_message(
        role: &str,
        content: &str,
        tool_calls: Option<Vec<serde_json::Value>>,
        tool_results: Option<Vec<serde_json::Value>>,
    ) -> MessageRecord {
        MessageRecord {
            id: Thing::from(("message", "test")),
            session: Thing::from(("session", "test")),
            role: role.to_string(),
            content: content.to_string(),
            tool_calls,
            tool_results,
            raw_output: None,
            created_at: Datetime::default(),
        }
    }

    #[test]
    fn transcript_preserves_user_and_assistant_text() {
        let messages = vec![
            make_message("user", "Hello", None, None),
            make_message("assistant", "Hi there!", None, None),
        ];

        let result = filter_transcript(&messages);
        assert!(result.contains("[user] Hello"));
        assert!(result.contains("[assistant] Hi there!"));
    }

    #[test]
    fn transcript_preserves_tool_calls() {
        let tool_call = serde_json::json!({
            "name": "read_file",
            "input": {"path": "/tmp/test.txt"}
        });
        let messages = vec![make_message("assistant", "", Some(vec![tool_call]), None)];

        let result = filter_transcript(&messages);
        assert!(result.contains("[tool_call] read_file("));
        assert!(result.contains("/tmp/test.txt"));
    }

    #[test]
    fn transcript_strips_tool_results() {
        let tool_result = serde_json::json!({
            "tool_use_id": "123",
            "content": "file contents here very long..."
        });
        let messages = vec![
            make_message("user", "Do something", None, None),
            make_message("user", "", None, Some(vec![tool_result])),
        ];

        let result = filter_transcript(&messages);
        assert!(result.contains("[user] Do something"));
        // Tool result message should be stripped
        assert!(!result.contains("file contents here"));
    }

    #[test]
    fn transcript_truncates_long_tool_inputs() {
        let long_input = "x".repeat(300);
        let tool_call = serde_json::json!({
            "name": "write_file",
            "input": {"content": long_input}
        });
        let messages = vec![make_message("assistant", "", Some(vec![tool_call]), None)];

        let result = filter_transcript(&messages);
        assert!(result.contains("..."));
    }

    #[test]
    fn web_cache_clearing() {
        let dir = TempDir::new().unwrap();
        let cache_dir = dir.path().join(".web-cache");
        std::fs::create_dir_all(&cache_dir).unwrap();

        std::fs::write(cache_dir.join("file1.md"), "content").unwrap();
        std::fs::write(cache_dir.join("file2.md"), "content").unwrap();

        assert!(cache_dir.join("file1.md").exists());

        clear_web_cache(dir.path()).unwrap();

        assert!(!cache_dir.join("file1.md").exists());
        assert!(!cache_dir.join("file2.md").exists());
        // Directory itself should still exist
        assert!(cache_dir.exists());
    }

    #[test]
    fn web_cache_clearing_missing_dir_is_ok() {
        let dir = TempDir::new().unwrap();
        assert!(clear_web_cache(dir.path()).is_ok());
    }

    #[test]
    fn load_state_file_returns_content() {
        let dir = TempDir::new().unwrap();
        let state_dir = dir.path().join(".state");
        std::fs::create_dir_all(&state_dir).unwrap();
        std::fs::write(state_dir.join("test.md"), "handoff note").unwrap();

        let result = load_state_file(dir.path(), "test.md");
        assert_eq!(result, Some("handoff note".to_string()));
    }

    #[test]
    fn load_state_file_returns_none_when_missing() {
        let dir = TempDir::new().unwrap();
        let result = load_state_file(dir.path(), "nonexistent.md");
        assert_eq!(result, None);
    }

    #[test]
    fn load_state_file_returns_none_when_empty() {
        let dir = TempDir::new().unwrap();
        let state_dir = dir.path().join(".state");
        std::fs::create_dir_all(&state_dir).unwrap();
        std::fs::write(state_dir.join("empty.md"), "  \n  ").unwrap();

        let result = load_state_file(dir.path(), "empty.md");
        assert_eq!(result, None);
    }

    #[test]
    fn build_user_message_includes_all_sections() {
        let msg = build_reflection_user_message(
            "Previous handoff content",
            "Diary entry today",
            "[user] Hello\n[assistant] Hi",
            "file1.md\nfile2.md",
        );
        assert!(msg.contains("## Previous Handoff Note"));
        assert!(msg.contains("Previous handoff content"));
        assert!(msg.contains("## Today's Diary"));
        assert!(msg.contains("Diary entry today"));
        assert!(msg.contains("## Conversation Transcript"));
        assert!(msg.contains("[user] Hello"));
        assert!(msg.contains("## Web Cache Files"));
        assert!(msg.contains("file1.md"));
    }

    #[test]
    fn build_user_message_defaults() {
        let msg = build_reflection_user_message(
            "No previous handoff.",
            "No diary entry for today.",
            "",
            "No cached files.",
        );
        assert!(msg.contains("No previous handoff."));
        assert!(msg.contains("No diary entry for today."));
        assert!(msg.contains("No cached files."));
    }
}
