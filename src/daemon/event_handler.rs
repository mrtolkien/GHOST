use std::path::PathBuf;
use std::sync::Arc;

use tokio::task::JoinHandle;

use crate::chat::SessionChat;
use crate::coding::prompt::build_coding_prompt;
use crate::db;
use crate::db::GhostDb;
use crate::events::{SessionEvent, SessionEventReceiver};
use crate::interfaces::discord::DiscordSender;

use crate::constants::{IDLE_POLL_INTERVAL, MAX_IDLE_POLLS};

/// Spawn a unified event handler that receives `SessionEvent`s and triggers
/// continuation chat turns. Replaces both `completion_watcher` and agent
/// `watcher`.
pub fn spawn_event_handler(
    mut rx: SessionEventReceiver,
    session_chat: Arc<SessionChat>,
    discord_sender: Option<Arc<DiscordSender>>,
    db: GhostDb,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) -> JoinHandle<()> {
    tracing::info!("session event handler started");

    tokio::spawn(async move {
        loop {
            tokio::select! {
                Some(event) = rx.recv() => {
                    Box::pin(handle_event(
                        event,
                        &session_chat,
                        discord_sender.as_deref(),
                        &db,
                    )).await;
                }
                _ = shutdown.changed() => {
                    tracing::info!("session event handler shutting down");
                    break;
                }
            }
        }
    })
}

async fn handle_event(
    event: SessionEvent,
    session_chat: &SessionChat,
    discord_sender: Option<&DiscordSender>,
    db: &GhostDb,
) {
    let session_id = &event.session_id;

    tracing::info!(session_id = session_id.clone(), "handling session event");

    // Wait for the session to be idle before triggering continuation.
    if !wait_for_idle(db, session_id).await {
        tracing::warn!(
            session_id = session_id.clone(),
            "session not idle after max polls, triggering anyway",
        );
    }

    // Resolve Discord channel: first via interface_sessions, fallback to
    // coding_sessions.channel_id.
    let discord_channel_id = resolve_discord_channel(db, session_id).await;

    // Send optional agent summary embed to Discord.
    if let Some(ref discord) = event.discord
        && let Some(ref agent_name) = discord.agent_name
        && let Some(ref metadata) = discord.agent_metadata
        && let Some(sender) = discord_sender
        && let Some(channel_id) = discord_channel_id
    {
        let summary = crate::interfaces::discord::ui_events::format_agent_summary(
            agent_name,
            metadata,
            discord.agent_findings.as_deref(),
        );
        let _ = Box::pin(sender.send_compact_container(channel_id, &summary, None)).await;
    }

    // Determine if this is a coding session and trigger the appropriate chat.
    let trigger = "[system] Background task completed.";
    let channel_id_str = discord_channel_id.map(|id| id.to_string());
    let chat_result = match detect_coding_session(db, session_chat, session_id).await {
        Some((working_dir, system_prompt)) => {
            tracing::info!(
                session_id = session_id.clone(),
                working_dir = working_dir.display().to_string(),
                "triggering coding continuation",
            );
            session_chat
                .chat_coding(
                    session_id,
                    trigger,
                    &system_prompt,
                    &working_dir,
                    channel_id_str,
                    None,
                )
                .await
        }
        None => {
            tracing::info!(
                session_id = session_id.clone(),
                "triggering GHOST continuation",
            );
            session_chat
                .chat(session_id, trigger, channel_id_str, None)
                .await
        }
    };

    // If the session is already being handled (e.g. by a Discord message),
    // the running tool loop will see the background task's system message
    // in its history. No need to trigger a separate response.
    if matches!(
        &chat_result,
        Err(crate::chat::ChatError::SessionBusy { .. })
    ) {
        tracing::info!(
            session_id = session_id.clone(),
            "session already active, skipping continuation",
        );
        return;
    }

    // Send response to Discord.
    match chat_result {
        Ok((result, metadata)) => {
            let statusline = crate::interfaces::discord::ui_events::format_statusline(&metadata);
            if let Some(sender) = discord_sender
                && let Some(channel_id) = discord_channel_id
                && let Err(e) = Box::pin(sender.send_to_channel_with_suffix(
                    channel_id,
                    &result.message,
                    &statusline,
                ))
                .await
            {
                tracing::error!(
                    error = e.to_string(),
                    "failed to send event response to Discord",
                );
            }
        }
        Err(e) => {
            tracing::error!(
                error = e.to_string(),
                session_id = session_id.clone(),
                "failed to trigger chat turn after session event",
            );
        }
    }
}

/// Poll until the session's latest message indicates no in-flight chat turn.
/// Accepts any system message as idle (the producer injects the system message
/// before sending the event) or an assistant message with no tool calls.
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

            // Any system message means no chat turn is in flight.
            if last.role == "system" {
                return true;
            }
        }

        tokio::time::sleep(IDLE_POLL_INTERVAL).await;
    }

    false
}

/// Resolve a Discord channel ID for the session. Tries `interface_sessions`
/// first, then falls back to `coding_sessions.channel_id`.
async fn resolve_discord_channel(db: &GhostDb, session_id: &str) -> Option<u64> {
    // Primary: interface_sessions table
    let interface = db::sessions::get_interface_for_session(db, session_id)
        .await
        .ok()
        .flatten();
    if let Some(channel_id) = interface.as_deref().and_then(parse_discord_channel_id) {
        return Some(channel_id);
    }

    // Fallback: coding_sessions.channel_id
    if let Ok(Some((_working_dir, Some(channel_str)))) =
        db::coding_sessions::get_coding_session_for_chat_session(db, session_id).await
        && let Some(channel_id) = parse_discord_channel_id(&channel_str)
    {
        return Some(channel_id);
    }

    None
}

/// Check if the session belongs to an active coding session. If so, return
/// the working directory and a built coding prompt.
async fn detect_coding_session(
    db: &GhostDb,
    session_chat: &SessionChat,
    session_id: &str,
) -> Option<(PathBuf, String)> {
    let (working_dir_str, _channel_id) =
        db::coding_sessions::get_coding_session_for_chat_session(db, session_id)
            .await
            .ok()
            .flatten()?;

    let working_dir = PathBuf::from(&working_dir_str);
    let config = session_chat.config();
    let system_prompt = build_coding_prompt(&config, &working_dir);
    Some((working_dir, system_prompt))
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
