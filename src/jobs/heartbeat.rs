use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use surrealdb::Surreal;
use surrealdb::engine::local::Db;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::agents::TaskRunner;
use crate::config::Config;
use crate::db;
use crate::db::sessions::MessageRecord;
use crate::interfaces::discord::DiscordSender;

use super::reflection::ReflectionManager;

pub struct HeartbeatManager {
    db: Surreal<Db>,
    task_runner: Arc<TaskRunner>,
    discord_sender: Arc<DiscordSender>,
    config: Config,
    reflection: Arc<ReflectionManager>,
    cooldowns: HashMap<String, DateTime<Utc>>,
}

impl HeartbeatManager {
    #[must_use]
    pub fn new(
        db: Surreal<Db>,
        task_runner: Arc<TaskRunner>,
        discord_sender: Arc<DiscordSender>,
        config: Config,
        reflection: Arc<ReflectionManager>,
    ) -> Self {
        Self {
            db,
            task_runner,
            discord_sender,
            config,
            reflection,
            cooldowns: HashMap::new(),
        }
    }

    pub fn spawn(mut self, mut shutdown: watch::Receiver<bool>) -> JoinHandle<()> {
        let check_secs = self.config.timing.heartbeat_check_seconds;
        logfire::info!("heartbeat manager started", check_seconds = check_secs,);

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(check_secs));

            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        self.check_idle_sessions().await;
                    }
                    _ = shutdown.changed() => {
                        logfire::info!("heartbeat manager shutting down");
                        break;
                    }
                }
            }
        })
    }

    async fn check_idle_sessions(&mut self) {
        let sessions = match db::interface_sessions::list_all_interface_sessions(&self.db).await {
            Ok(s) => s,
            Err(e) => {
                logfire::warn!("heartbeat: failed to list sessions", error = e.to_string(),);
                return;
            }
        };

        let now = Utc::now();
        let idle_threshold =
            chrono::Duration::minutes(self.config.timing.heartbeat_idle_minutes as i64);
        let cooldown_duration =
            chrono::Duration::minutes(self.config.timing.heartbeat_continue_minutes as i64);

        for record in sessions {
            let session_id = crate::db::fmt_id(&record.session);

            // Check cooldown
            if let Some(last_hb) = self.cooldowns.get(&session_id)
                && now - *last_hb < cooldown_duration
            {
                continue;
            }

            // Load session to check last_activity_at
            let session = match db::sessions::get_session(&self.db, &record.session).await {
                Ok(s) => s,
                Err(e) => {
                    logfire::warn!(
                        "heartbeat: failed to load session",
                        session_id = session_id.clone(),
                        error = e.to_string(),
                    );
                    continue;
                }
            };

            if session.status != "active" {
                continue;
            }

            let last_activity: DateTime<Utc> = *session.last_activity_at;
            let idle_time = now - last_activity;

            if idle_time < idle_threshold {
                continue;
            }

            // Skip if no messages since last heartbeat
            let since = self
                .cooldowns
                .get(&session_id)
                .copied()
                .unwrap_or(last_activity);
            match db::sessions::count_messages_since(&self.db, &record.session, &since).await {
                Ok(0) => continue,
                Ok(_) => {}
                Err(e) => {
                    logfire::warn!(
                        "heartbeat: failed to count messages",
                        session_id = session_id.clone(),
                        error = e.to_string(),
                    );
                    continue;
                }
            }

            self.run_heartbeat(&session_id, &record.interface, &record.session)
                .await;
        }
    }

    #[tracing::instrument(name = "heartbeat", skip_all, fields(session_id = ?session_id, interface = %interface))]
    async fn run_heartbeat(
        &mut self,
        session_id: &str,
        interface: &str,
        session_thing: &surrealdb::types::RecordId,
    ) {
        logfire::info!("heartbeat started", session_id = session_id.to_string(),);

        // Build user message with recent conversation context
        let user_message = match self.build_user_message(session_thing).await {
            Ok(msg) => msg,
            Err(e) => {
                logfire::error!(
                    "heartbeat: failed to build user message",
                    error = e.to_string(),
                );
                return;
            }
        };

        let response = match self
            .task_runner
            .run_to_completion("heartbeat", &user_message, Some(session_thing))
            .await
        {
            Ok(findings) => findings,
            Err(e) => {
                logfire::error!(
                    "heartbeat agent failed",
                    session_id = session_id.to_string(),
                    error = e.to_string(),
                );
                return;
            }
        };

        // Save response to .state/heartbeat.last.md
        let state_dir = self.config.workspace.join(".state");
        let _ = std::fs::create_dir_all(&state_dir);
        let state_path = state_dir.join("heartbeat.last.md");
        if let Err(e) = std::fs::write(&state_path, &response) {
            logfire::warn!(
                "heartbeat: failed to write state file",
                error = e.to_string(),
            );
        }

        let now = Utc::now();

        if is_heartbeat_continue(&response) {
            self.cooldowns.insert(session_id.to_string(), now);
            logfire::info!(
                "heartbeat completed",
                session_id = session_id.to_string(),
                outcome = "suppressed",
            );
        } else {
            // Send to Discord channel
            if let Some(channel_id) = extract_discord_channel_id(interface) {
                if let Err(e) = self
                    .discord_sender
                    .send_to_channel(channel_id, &response)
                    .await
                {
                    logfire::error!(
                        "heartbeat: failed to send to Discord",
                        session_id = session_id.to_string(),
                        channel_id = channel_id,
                        error = e.to_string(),
                    );
                }
            } else {
                logfire::warn!(
                    "heartbeat: could not extract channel ID",
                    interface = interface.to_string(),
                );
            }
            self.cooldowns.insert(session_id.to_string(), now);
            logfire::info!(
                "heartbeat completed",
                session_id = session_id.to_string(),
                outcome = "sent",
            );
        }

        // Trigger reflection after heartbeat
        let reflection = self.reflection.clone();
        let sid = session_id.to_string();
        let st = session_thing.clone();
        tokio::spawn(async move {
            reflection.run_after_heartbeat(&sid, &st).await;
        });
    }

    async fn build_user_message(
        &self,
        session_thing: &surrealdb::types::RecordId,
    ) -> Result<String, db::DatabaseError> {
        let messages = db::sessions::list_messages_by_session(&self.db, session_thing).await?;
        let transcript = format_recent_messages(&messages, 20);
        Ok(build_heartbeat_user_message(&transcript))
    }
}

/// Build the user message for the heartbeat agent.
#[must_use]
pub fn build_heartbeat_user_message(recent_messages: &str) -> String {
    format!(
        "## Recent Conversation\n\
         {recent_messages}"
    )
}

/// Format recent messages for heartbeat context.
///
/// Takes the last `max` messages, preserving user and assistant text.
/// Tool calls and results are summarized briefly.
#[must_use]
pub fn format_recent_messages(messages: &[MessageRecord], max: usize) -> String {
    let start = messages.len().saturating_sub(max);
    let mut lines = Vec::new();

    for msg in &messages[start..] {
        match msg.role.as_str() {
            "user" => {
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
                if let Some(ref calls) = msg.tool_calls {
                    for call in calls {
                        let name = call
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown");
                        lines.push(format!("[tool_call] {name}"));
                    }
                }
            }
            _ => {}
        }
    }

    if lines.is_empty() {
        "No recent messages.".to_string()
    } else {
        lines.join("\n")
    }
}

/// Check whether the response is a HEARTBEAT_CONTINUE signal.
///
/// The response must contain HEARTBEAT_CONTINUE as a standalone marker.
/// If the response contains substantial other text, it's treated as a real
/// message to send.
pub fn is_heartbeat_continue(response: &str) -> bool {
    let trimmed = response.trim();
    trimmed.eq_ignore_ascii_case("HEARTBEAT_CONTINUE")
}

/// Extract a Discord channel ID from an interface key like
/// `discord:channel:123456789`.
pub fn extract_discord_channel_id(interface: &str) -> Option<u64> {
    interface
        .strip_prefix("discord:channel:")
        .and_then(|id_str| id_str.parse::<u64>().ok())
}

/// Load a prompt from workspace override or fall back to embedded default.
pub fn load_prompt(workspace: &Path, filename: &str, default: &str) -> String {
    let override_path = workspace.join(filename);
    match std::fs::read_to_string(&override_path) {
        Ok(content) if !content.trim().is_empty() => content,
        _ => default.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use surrealdb::types::{Datetime, RecordId};
    use tempfile::TempDir;

    fn make_message(role: &str, content: &str) -> MessageRecord {
        MessageRecord {
            id: RecordId::new("message", "test"),
            session: RecordId::new("session", "test"),
            role: role.to_string(),
            content: content.to_string(),
            tool_calls: None,
            tool_results: None,
            raw_output: None,
            created_at: Datetime::default(),
        }
    }

    #[test]
    fn heartbeat_continue_exact_match() {
        assert!(is_heartbeat_continue("HEARTBEAT_CONTINUE"));
    }

    #[test]
    fn heartbeat_continue_case_insensitive() {
        assert!(is_heartbeat_continue("heartbeat_continue"));
        assert!(is_heartbeat_continue("Heartbeat_Continue"));
    }

    #[test]
    fn heartbeat_continue_with_whitespace() {
        assert!(is_heartbeat_continue("  HEARTBEAT_CONTINUE  "));
        assert!(is_heartbeat_continue("\nHEARTBEAT_CONTINUE\n"));
    }

    #[test]
    fn heartbeat_continue_embedded_in_text_is_not_continue() {
        assert!(!is_heartbeat_continue(
            "I have nothing to say. HEARTBEAT_CONTINUE"
        ));
        assert!(!is_heartbeat_continue(
            "HEARTBEAT_CONTINUE but also here's a thought"
        ));
    }

    #[test]
    fn heartbeat_continue_empty_is_not_continue() {
        assert!(!is_heartbeat_continue(""));
        assert!(!is_heartbeat_continue("   "));
    }

    #[test]
    fn extract_channel_id_valid() {
        assert_eq!(
            extract_discord_channel_id("discord:channel:123456789"),
            Some(123456789)
        );
    }

    #[test]
    fn extract_channel_id_invalid_prefix() {
        assert_eq!(extract_discord_channel_id("slack:channel:123"), None);
        assert_eq!(extract_discord_channel_id("discord"), None);
    }

    #[test]
    fn extract_channel_id_non_numeric() {
        assert_eq!(extract_discord_channel_id("discord:channel:abc"), None);
    }

    #[test]
    fn prompt_loading_uses_workspace_override() {
        let dir = TempDir::new().unwrap();
        let override_content = "Custom heartbeat prompt";
        std::fs::write(dir.path().join("heartbeat.md"), override_content).unwrap();

        let prompt = load_prompt(dir.path(), "heartbeat.md", "default");
        assert_eq!(prompt, override_content);
    }

    #[test]
    fn prompt_loading_falls_back_to_default() {
        let dir = TempDir::new().unwrap();
        let prompt = load_prompt(dir.path(), "heartbeat.md", "default prompt");
        assert_eq!(prompt, "default prompt");
    }

    #[test]
    fn prompt_loading_ignores_empty_override() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("heartbeat.md"), "   \n  ").unwrap();

        let prompt = load_prompt(dir.path(), "heartbeat.md", "default prompt");
        assert_eq!(prompt, "default prompt");
    }

    #[test]
    fn format_recent_messages_basic() {
        let messages = vec![
            make_message("user", "Hello"),
            make_message("assistant", "Hi there!"),
        ];
        let result = format_recent_messages(&messages, 20);
        assert!(result.contains("[user] Hello"));
        assert!(result.contains("[assistant] Hi there!"));
    }

    #[test]
    fn format_recent_messages_limits_count() {
        let messages = vec![
            make_message("user", "First"),
            make_message("assistant", "Second"),
            make_message("user", "Third"),
        ];
        let result = format_recent_messages(&messages, 2);
        assert!(!result.contains("First"));
        assert!(result.contains("Second"));
        assert!(result.contains("Third"));
    }

    #[test]
    fn format_recent_messages_empty() {
        let result = format_recent_messages(&[], 20);
        assert_eq!(result, "No recent messages.");
    }

    #[test]
    fn build_heartbeat_user_message_includes_section() {
        let msg = build_heartbeat_user_message("[user] Hello\n[assistant] Hi");
        assert!(msg.contains("## Recent Conversation"));
        assert!(msg.contains("[user] Hello"));
    }
}
