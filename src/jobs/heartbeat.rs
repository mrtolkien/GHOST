use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use surrealdb::Surreal;
use surrealdb::engine::local::Db;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::chat::SessionChat;
use crate::config::Config;
use crate::db;
use crate::interfaces::discord::DiscordSender;

use super::reflection::ReflectionManager;

const DEFAULT_HEARTBEAT_PROMPT: &str = include_str!("../../prompts/heartbeat.md");

pub struct HeartbeatManager {
    db: Surreal<Db>,
    session_chat: Arc<SessionChat>,
    discord_sender: Arc<DiscordSender>,
    config: Config,
    reflection: Arc<ReflectionManager>,
    cooldowns: HashMap<String, DateTime<Utc>>,
}

impl HeartbeatManager {
    #[must_use]
    pub fn new(
        db: Surreal<Db>,
        session_chat: Arc<SessionChat>,
        discord_sender: Arc<DiscordSender>,
        config: Config,
        reflection: Arc<ReflectionManager>,
    ) -> Self {
        Self {
            db,
            session_chat,
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

    #[tracing::instrument(skip_all)]
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
            let session_id = record.session.to_string();

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

            let last_activity: DateTime<Utc> = session.last_activity_at.0;
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

    #[tracing::instrument(skip_all, fields(session_id = %session_id, interface = %interface))]
    async fn run_heartbeat(
        &mut self,
        session_id: &str,
        interface: &str,
        session_thing: &surrealdb::sql::Thing,
    ) {
        logfire::info!("heartbeat started", session_id = session_id.to_string(),);

        let prompt = load_prompt(
            &self.config.workspace,
            "heartbeat.md",
            DEFAULT_HEARTBEAT_PROMPT,
        );

        let result = match self
            .session_chat
            .chat_job(
                "heartbeat",
                session_id,
                &prompt,
                crate::tools::ToolSet::Chat,
            )
            .await
        {
            Ok(t) => t,
            Err(e) => {
                logfire::error!(
                    "heartbeat chat_job failed",
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
        if let Err(e) = std::fs::write(&state_path, &result.result.message) {
            logfire::warn!(
                "heartbeat: failed to write state file",
                error = e.to_string(),
            );
        }

        let now = Utc::now();

        if is_heartbeat_continue(&result.result.message) {
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
                    .send_to_channel(channel_id, &result.result.message)
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
    use tempfile::TempDir;

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
}
