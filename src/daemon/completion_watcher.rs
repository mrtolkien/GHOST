use std::sync::Arc;
use std::time::Duration;

use tokio::task::JoinHandle;

use crate::chat::SessionChat;
use crate::completion::{CompletionEvent, CompletionReceiver};
use crate::db;
use crate::db::GhostDb;
use crate::interfaces::discord::DiscordSender;

const IDLE_POLL_INTERVAL: Duration = Duration::from_secs(2);
const MAX_IDLE_POLLS: usize = 30;

/// Spawn a task that watches for background completion events and triggers
/// follow-up chat turns. Mirrors the agent watcher pattern.
pub fn spawn_completion_watcher(
    mut rx: CompletionReceiver,
    session_chat: Arc<SessionChat>,
    discord_sender: Option<Arc<DiscordSender>>,
    db: GhostDb,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) -> JoinHandle<()> {
    logfire::info!("completion watcher started");

    tokio::spawn(async move {
        loop {
            tokio::select! {
                Some(event) = rx.recv() => {
                    handle_event(
                        event,
                        &session_chat,
                        discord_sender.as_deref(),
                        &db,
                    ).await;
                }
                _ = shutdown.changed() => {
                    logfire::info!("completion watcher shutting down");
                    break;
                }
            }
        }
    })
}

async fn handle_event(
    event: CompletionEvent,
    session_chat: &SessionChat,
    discord_sender: Option<&DiscordSender>,
    db: &GhostDb,
) {
    match event {
        CompletionEvent::ShellCommand {
            session_id,
            command,
        } => {
            logfire::info!(
                "background shell command completed, triggering continuation",
                session_id = session_id.clone(),
                command = command.clone(),
            );

            // Wait for the session to be idle (last message is an assistant
            // message with no tool_calls). This prevents racing with an
            // in-flight chat turn on the same session.
            if !wait_for_idle(db, &session_id).await {
                logfire::warn!(
                    "session not idle after max polls, triggering anyway",
                    session_id = session_id.clone(),
                );
            }

            // Resolve Discord channel for this session
            let channel = db::sessions::get_interface_for_session(db, &session_id)
                .await
                .ok()
                .flatten();
            let discord_channel_id = channel.as_deref().and_then(parse_discord_channel_id);

            // Trigger a continuation chat turn
            let trigger = "[system] Background command completed.";
            match session_chat.chat(&session_id, trigger, None).await {
                Ok((result, _metadata)) => {
                    if let Some(sender) = discord_sender
                        && let Some(channel_id) = discord_channel_id
                        && let Err(e) = sender.send_to_channel(channel_id, &result.message).await
                    {
                        logfire::error!(
                            "failed to send completion response to Discord",
                            error = e.to_string(),
                        );
                    }
                }
                Err(e) => {
                    logfire::error!(
                        "failed to trigger chat turn after shell completion",
                        error = e.to_string(),
                    );
                }
            }
        }
    }
}

/// Poll until the session's latest message is an idle assistant message
/// (no pending tool calls). Returns `true` if idle was detected, `false`
/// if we hit the max poll limit.
async fn wait_for_idle(db: &GhostDb, session_id: &str) -> bool {
    for _ in 0..MAX_IDLE_POLLS {
        let messages = db::sessions::list_messages_by_session(db, session_id)
            .await
            .unwrap_or_default();

        if let Some(last) = messages.last() {
            let has_tool_calls = last.tool_calls_parsed().is_some_and(|tc| !tc.is_empty());

            if last.role == "assistant" && !has_tool_calls {
                return true;
            }

            // Also accept if the last message is the system message we just
            // wrote (the [shell-command completed] message) — means no chat
            // turn is in flight.
            if last.role == "system" && last.content.contains("[shell-command completed]") {
                return true;
            }
        }

        tokio::time::sleep(IDLE_POLL_INTERVAL).await;
    }

    false
}

/// Extract channel ID from an interface key like "discord:channel:123456".
fn parse_discord_channel_id(interface_key: &str) -> Option<u64> {
    interface_key
        .strip_prefix("discord:channel:")
        .and_then(|id| id.parse().ok())
}
