use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use surrealdb::Surreal;
use surrealdb::engine::local::Db;
use surrealdb::sql::Thing;
use tokio::sync::Mutex;

use crate::agents::ProgressRule;
use crate::chat::SessionChat;
use crate::config::Config;
use crate::db;
use crate::db::sessions::MessageRecord;
use crate::prompt::{JobPromptContext, PromptRenderer};
use crate::providers::provider_for_alias;
use crate::tools::{ToolManager, ToolSet};
use crate::web::scan_web_cache;

use super::heartbeat::load_prompt;

/// Progress rules for reflection: nudge the model to keep creating notes
/// and curating references instead of stopping early with a text handoff.
pub fn reflection_progress_rules() -> Vec<ProgressRule> {
    vec![
        ProgressRule {
            tool: "note_write".to_string(),
            min: 3,
            below: Some(
                "You have only created {count}/{min} notes. \
                 Keep going — create entity notes for each product/concept \
                 found in the transcript. Do NOT write your handoff yet."
                    .to_string(),
            ),
            met: None,
        },
        ProgressRule {
            tool: "reference_manage".to_string(),
            min: 3,
            below: Some(
                "You have only curated {count}/{min} references. \
                 Process more web cache files before moving on. \
                 Use reference_manage(action=\"move\") for useful files."
                    .to_string(),
            ),
            met: None,
        },
    ]
}

pub const DEFAULT_REFLECTION_PROMPT: &str = include_str!("../../prompts/reflection.md");

pub struct ReflectionManager {
    db: Surreal<Db>,
    config: Config,
    running: Arc<Mutex<()>>,
}

impl ReflectionManager {
    #[must_use]
    pub fn new(db: Surreal<Db>, config: Config) -> Self {
        Self {
            db,
            config,
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

        let prompt_body = load_prompt(
            &self.config.workspace,
            "reflection.md",
            DEFAULT_REFLECTION_PROMPT,
        );

        // Load template variables
        let previous_handoff = load_state_file(&self.config.workspace, "reflection.last.md");
        let diary_today = load_diary_today(&self.config.workspace);
        let recent_messages = match self.load_filtered_transcript(session_thing).await {
            Ok(t) => Some(t),
            Err(e) => {
                logfire::warn!(
                    "reflection: failed to load transcript",
                    error = e.to_string(),
                );
                None
            }
        };
        let web_cache_files = match scan_web_cache(&self.config.workspace) {
            Ok(files) => files,
            Err(e) => {
                logfire::warn!(
                    "reflection: failed to scan web cache",
                    error = e.to_string(),
                );
                None
            }
        };

        // Render prompt with template variables
        let renderer = PromptRenderer::new(self.config.clone());
        let interpolated = match renderer.render_job_prompt(
            "reflection",
            &JobPromptContext {
                prompt_body,
                previous_handoff,
                diary_today,
                recent_messages: recent_messages.clone(),
                web_cache_files,
            },
        ) {
            Ok(p) => p,
            Err(e) => {
                logfire::error!("reflection: failed to render prompt", error = e.to_string(),);
                return;
            }
        };

        // Create a SessionChat with reflection tools
        let provider = match provider_for_alias(&self.config, None) {
            Ok(p) => p,
            Err(e) => {
                logfire::error!("reflection: failed to init provider", error = e.to_string(),);
                return;
            }
        };

        let session_chat = SessionChat::new(
            self.db.clone(),
            provider,
            ToolManager::for_reflection(),
            self.config.clone(),
        );

        // Create a temporary session for the reflection job
        let temp_session = match db::sessions::create_session(&self.db).await {
            Ok(s) => s,
            Err(e) => {
                logfire::error!(
                    "reflection: failed to create temp session",
                    error = e.to_string(),
                );
                return;
            }
        };
        let temp_session_id = temp_session.to_string();

        match session_chat
            .chat_job_with_rules(
                "reflection",
                &temp_session_id,
                &interpolated,
                ToolSet::Reflection,
                reflection_progress_rules(),
            )
            .await
        {
            Ok(transcript) => {
                // Save handoff note
                let state_dir = self.config.workspace.join(".state");
                let _ = std::fs::create_dir_all(&state_dir);
                let state_path = state_dir.join("reflection.last.md");
                if let Err(e) = std::fs::write(&state_path, &transcript.result.message) {
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

    async fn load_filtered_transcript(
        &self,
        session_thing: &Thing,
    ) -> Result<String, db::DatabaseError> {
        let messages = db::sessions::list_messages_by_session(&self.db, session_thing).await?;
        Ok(filter_transcript(&messages))
    }
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
}
