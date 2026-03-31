use std::path::PathBuf;
use std::sync::Arc;

use tokio::task::JoinHandle;

use crate::chat::SessionChat;
use crate::chat::interrupt::ActiveSessions;
use crate::coding::prompt::build_coding_prompt;
use crate::db;
use crate::db::GhostDb;
use crate::events::{SessionEvent, SessionEventReceiver};
use crate::interfaces::discord::{DiscordSender, TimedTyping};
use serenity::model::id::ChannelId;

use crate::constants::{IDLE_POLL_INTERVAL, MAX_DISCORD_SYSTEM_MESSAGE_CHARS, MAX_IDLE_POLLS};

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

    tracing::info!(
        session_id = session_id.clone(),
        notify_only = event.notify_only,
        "handling session event",
    );

    // Resolve Discord channel: first via interface_sessions, fallback to
    // coding_sessions.channel_id.
    let discord_channel_id = resolve_discord_channel(db, session_id).await;

    // For notify-only events, deliver the message content to Discord and stop —
    // no idle-wait needed since we don't trigger a continuation turn.
    if event.notify_only {
        if let Some(sender) = discord_sender
            && let Some(channel_id) = discord_channel_id
            && let Err(e) =
                Box::pin(sender.send_to_channel(channel_id, &event.system_message)).await
        {
            tracing::error!(
                error = e.to_string(),
                "failed to send notify_only message to Discord",
            );
        }
        return;
    }

    // Start typing indicator so users see activity during the idle wait + chat
    // turn. The `_typing` handle auto-stops on drop (when this function returns).
    let _typing = discord_sender
        .zip(discord_channel_id)
        .map(|(sender, ch)| TimedTyping::start(ChannelId::new(ch), sender.http()));

    // Wait for any in-flight tool loop to finish before triggering continuation.
    let active_sessions = session_chat.active_sessions();
    if !wait_for_idle(active_sessions, session_id).await {
        tracing::warn!(
            session_id = session_id.clone(),
            "session not idle after max polls, triggering anyway",
        );
    }

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

    // Send the background command output to Discord so the user sees what
    // triggered the continuation turn. Truncate to fit Discord's text_display
    // limit (send_gateway_v2 does not chunk automatically).
    if let Some(sender) = discord_sender
        && let Some(channel_id) = discord_channel_id
    {
        let content = truncate_for_discord(&event.system_message);
        if let Err(e) = Box::pin(sender.send_system_message(channel_id, &content, None)).await {
            tracing::error!(
                error = e.to_string(),
                "failed to send background output to Discord",
            );
        }
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
                    channel_id_str.clone(),
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
                .chat(session_id, trigger, channel_id_str.clone(), None)
                .await
        }
    };

    // If the session became busy between our idle check and the chat() call
    // (e.g. the user sent a new message), wait again and retry once. The
    // previous code assumed the running tool loop would see the system message,
    // but that's false when the model is mid-generation and about to end_turn.
    let chat_result = if matches!(
        &chat_result,
        Err(crate::chat::ChatError::SessionBusy { .. })
    ) {
        tracing::info!(
            session_id = session_id.clone(),
            "session busy, waiting for idle to retry continuation",
        );
        if !wait_for_idle(active_sessions, session_id).await {
            tracing::warn!(
                session_id = session_id.clone(),
                "session still not idle after retry, triggering anyway",
            );
        }
        match detect_coding_session(db, session_chat, session_id).await {
            Some((working_dir, system_prompt)) => {
                session_chat
                    .chat_coding(
                        session_id,
                        trigger,
                        &system_prompt,
                        &working_dir,
                        channel_id_str.clone(),
                        None,
                    )
                    .await
            }
            None => {
                session_chat
                    .chat(session_id, trigger, channel_id_str.clone(), None)
                    .await
            }
        }
    } else {
        chat_result
    };

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

/// Poll until no tool loop is running for this session.
///
/// The `active_sessions` DashMap is the authoritative source of truth — a
/// session is busy iff it has an entry. The previous DB-message heuristic was
/// wrong: a system message injected mid-loop doesn't mean the session is idle.
async fn wait_for_idle(active_sessions: &ActiveSessions, session_id: &str) -> bool {
    for _ in 0..MAX_IDLE_POLLS {
        if !active_sessions.contains_key(session_id) {
            return true;
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

/// Truncate a system message to fit within Discord's v2 text_display limit.
fn truncate_for_discord(content: &str) -> String {
    if content.len() <= MAX_DISCORD_SYSTEM_MESSAGE_CHARS {
        return content.to_string();
    }
    let end = content.floor_char_boundary(MAX_DISCORD_SYSTEM_MESSAGE_CHARS);
    let mut truncated = content[..end].to_string();
    truncated.push_str("\n...[truncated]");
    truncated
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

    #[test]
    fn truncate_short_message_unchanged() {
        let msg = "hello world";
        assert_eq!(truncate_for_discord(msg), msg);
    }

    #[test]
    fn truncate_long_message() {
        let msg = "a".repeat(MAX_DISCORD_SYSTEM_MESSAGE_CHARS + 500);
        let result = truncate_for_discord(&msg);
        assert!(result.len() <= MAX_DISCORD_SYSTEM_MESSAGE_CHARS + 20);
        assert!(result.ends_with("...[truncated]"));
    }
}
