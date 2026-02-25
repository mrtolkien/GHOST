use std::sync::Arc;
use std::time::Duration;

use surrealdb::Surreal;
use surrealdb::engine::local::Db;
use surrealdb::types::RecordId;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::chat::SessionChat;
use crate::db;
use crate::interfaces::discord::DiscordSender;
use crate::jobs::ReflectionManager;

use super::runner::TaskRunner;

const POLL_INTERVAL_SECS: u64 = 3;

/// Poll for completed agents and inject their findings into parent sessions.
pub fn spawn_task_watcher(
    task_runner: Arc<TaskRunner>,
    session_chat: Arc<SessionChat>,
    discord_sender: Arc<DiscordSender>,
    reflection: Arc<ReflectionManager>,
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
                        &reflection,
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
    reflection: &Arc<ReflectionManager>,
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

        // Resolve Discord channel for this parent session
        let channel = db::sessions::get_interface_for_session(db, &parent_id)
            .await
            .ok()
            .flatten();
        let discord_channel_id = channel.as_deref().and_then(parse_discord_channel_id);

        // Send compact agent summary to Discord
        if let Some(channel_id) = discord_channel_id
            && let Some(ref metadata) = status.metadata
        {
            let findings_snippet = status.findings.as_deref();
            let summary = crate::interfaces::discord::ui_events::format_agent_summary(
                &status.agent_name,
                metadata,
                findings_snippet,
            );
            let _ = discord_sender
                .send_compact_container(channel_id, &summary, None)
                .await;
        }

        // Trigger a new chat turn with a synthetic user message
        let trigger = "[system] Research agent completed.";
        match session_chat
            .chat(&crate::db::fmt_id(&parent_id), trigger, None)
            .await
        {
            Ok((result, _metadata)) => {
                if let Some(channel_id) = discord_channel_id
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

        // Spawn agent reflection on the agent's own session (skip self-reflection)
        if !status.agent_name.contains("reflection")
            && let Some(thing) = parse_agent_session_thing(&agent_id)
        {
            let reflection = Arc::clone(reflection);
            tokio::spawn(async move {
                reflection.run_after_agent_handoff(&thing).await;
            });
        }
    }
}

/// Parse "session:abc123" into a RecordId.
fn parse_agent_session_thing(agent_id: &str) -> Option<RecordId> {
    let (table, id) = agent_id.split_once(':')?;
    if table.is_empty() || id.is_empty() {
        return None;
    }
    Some(RecordId::new(table, id))
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
