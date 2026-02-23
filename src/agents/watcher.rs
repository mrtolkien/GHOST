use std::sync::Arc;
use std::time::Duration;

use surrealdb::Surreal;
use surrealdb::engine::local::Db;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::chat::SessionChat;
use crate::db;
use crate::interfaces::discord::DiscordSender;

use super::runner::TaskRunner;

const POLL_INTERVAL_SECS: u64 = 3;

/// Poll for completed agents and inject their findings into parent sessions.
pub fn spawn_task_watcher(
    task_runner: Arc<TaskRunner>,
    session_chat: Arc<SessionChat>,
    discord_sender: Arc<DiscordSender>,
    db: Surreal<Db>,
    mut shutdown: watch::Receiver<bool>,
) -> JoinHandle<()> {
    logfire::info!("agent watcher started");

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(POLL_INTERVAL_SECS));

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    check_completed_tasks(
                        &task_runner,
                        &session_chat,
                        &discord_sender,
                        &db,
                    ).await;
                }
                _ = shutdown.changed() => {
                    logfire::info!("agent watcher shutting down");
                    break;
                }
            }
        }
    })
}

async fn check_completed_tasks(
    task_runner: &TaskRunner,
    session_chat: &SessionChat,
    discord_sender: &DiscordSender,
    db: &Surreal<Db>,
) {
    let agent_ids = task_runner.list_task_ids().await;

    for agent_id in agent_ids {
        let Some((status, parent_session)) = task_runner.take_completed(&agent_id).await else {
            continue;
        };

        logfire::info!(
            "agent completed, injecting findings",
            agent_name = status.agent_name.clone(),
            agent_id = status.agent_id.clone(),
        );

        let findings = status
            .findings
            .as_deref()
            .unwrap_or("Agent completed without producing findings.");

        let Some(parent_id) = parent_session else {
            logfire::warn!(
                "completed agent has no parent session, skipping injection",
                agent_id = status.agent_id.clone(),
            );
            continue;
        };

        // Inject findings as a system message in the parent session
        let system_msg = format!("[agent:{} completed]\n\n{findings}", status.agent_name);
        if let Err(e) = db::sessions::create_message(db, &parent_id, "system", &system_msg).await {
            logfire::error!(
                "failed to inject agent findings into parent session",
                error = e.to_string(),
            );
            continue;
        }

        // Trigger a new chat turn with a synthetic user message
        let trigger = "[system] Research agent completed.";
        match session_chat.chat(&parent_id.to_string(), trigger).await {
            Ok(result) => {
                // Send the response to the right Discord channel
                let channel = db::sessions::get_interface_for_session(db, &parent_id)
                    .await
                    .ok()
                    .flatten();

                if let Some(interface_key) = channel
                    && let Some(channel_id) = parse_discord_channel_id(&interface_key)
                    && let Err(e) = discord_sender
                        .send_to_channel(channel_id, &result.message)
                        .await
                {
                    logfire::error!(
                        "failed to send agent findings to Discord",
                        error = e.to_string(),
                    );
                }
            }
            Err(e) => {
                logfire::error!(
                    "failed to trigger chat turn after agent completion",
                    error = e.to_string(),
                );
            }
        }
    }
}

/// Extract channel ID from an interface key like "discord:channel:123456".
fn parse_discord_channel_id(interface_key: &str) -> Option<u64> {
    interface_key
        .strip_prefix("discord:channel:")
        .and_then(|id| id.parse().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_discord_channel_id_valid() {
        assert_eq!(
            parse_discord_channel_id("discord:channel:123456789"),
            Some(123456789)
        );
    }

    #[test]
    fn parse_discord_channel_id_invalid() {
        assert_eq!(parse_discord_channel_id("slack:channel:123"), None);
        assert_eq!(parse_discord_channel_id("discord:channel:"), None);
        assert_eq!(parse_discord_channel_id("garbage"), None);
    }
}
