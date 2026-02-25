use std::sync::{Arc, OnceLock};
use std::time::Duration;

use serenity::async_trait;
use serenity::http::Http;
use serenity::model::channel::Message;
use serenity::model::gateway::Ready;
use serenity::model::id::ChannelId;
use serenity::prelude::*;
use surrealdb::Surreal;
use surrealdb::engine::local::Db;
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

use crate::chat::{ChatStopReason, SessionChat};
use crate::config::Config;
use crate::db;

use super::send::{WARNING_EMBED_COLOR, send_assistant_v2, send_gateway_v2};
use super::ui_events::DiscordUiRenderer;
use super::ui_events::format_statusline;

/// Maximum duration for a typing indicator before it auto-stops.
const TYPING_TIMEOUT: Duration = Duration::from_secs(300);

/// Max file size for attachment downloads (25 MB).
const MAX_ATTACHMENT_SIZE: usize = 25 * 1024 * 1024;

/// File extensions treated as downloadable text files.
const TEXT_EXTENSIONS: &[&str] = &[
    "txt", "md", "rs", "py", "js", "ts", "json", "toml", "yaml", "yml", "sh", "css", "html", "xml",
    "sql", "csv", "log",
];

// ---------------------------------------------------------------------------
// TimedTyping — typing indicator with automatic timeout
// ---------------------------------------------------------------------------

struct TimedTyping {
    _handle: JoinHandle<()>,
}

impl TimedTyping {
    fn start(channel_id: ChannelId, http: &Arc<Http>) -> Self {
        let typing = channel_id.start_typing(http);
        let handle = tokio::spawn(async move {
            tokio::time::sleep(TYPING_TIMEOUT).await;
            drop(typing);
        });
        Self { _handle: handle }
    }
}

impl Drop for TimedTyping {
    fn drop(&mut self) {
        self._handle.abort();
    }
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

pub(super) struct Handler {
    session_chat: Arc<SessionChat>,
    db: Surreal<Db>,
    config: Config,
    allowed_user_id: String,
    bot_user_id: OnceLock<String>,
    started_at: std::time::SystemTime,
}

impl Handler {
    pub fn new(
        session_chat: Arc<SessionChat>,
        db: Surreal<Db>,
        config: Config,
        allowed_user_id: String,
    ) -> Self {
        Self {
            session_chat,
            db,
            config,
            allowed_user_id,
            bot_user_id: OnceLock::new(),
            started_at: std::time::SystemTime::now(),
        }
    }

    /// Strip `<@BOT_ID>` mentions from message content.
    fn strip_bot_mention<'a>(&self, content: &'a str) -> &'a str {
        let Some(bot_id) = self.bot_user_id.get() else {
            return content;
        };
        let mention = format!("<@{bot_id}>");
        content
            .strip_prefix(&mention)
            .unwrap_or(content)
            .trim_start()
    }

    /// Build the interface key for a Discord channel.
    fn interface_key(channel_id: ChannelId) -> String {
        format!("discord:channel:{}", channel_id)
    }

    /// Resolve or create a session for this channel.
    #[tracing::instrument(skip_all, level = "debug", fields(channel_id = %channel_id))]
    async fn resolve_session(
        &self,
        channel_id: ChannelId,
    ) -> Result<String, crate::db::DatabaseError> {
        let iface = Self::interface_key(channel_id);

        if let Some(thing) =
            db::interface_sessions::get_active_session_for_interface(&self.db, &iface).await?
        {
            return Ok(crate::db::fmt_id(&thing));
        }

        let new_session = db::sessions::create_session(&self.db).await?;
        db::interface_sessions::set_active_session_for_interface(&self.db, &iface, &new_session)
            .await?;
        let new_session_str = crate::db::fmt_id(&new_session);
        info!(
            session_id = %new_session_str,
            channel_id = %channel_id,
            "created new session for channel"
        );
        Ok(new_session_str)
    }

    /// Download text attachments to workspace/downloads/. Returns content to
    /// prepend to the user message.
    #[tracing::instrument(skip_all, level = "debug", fields(
        attachment_count = attachments.len()
    ))]
    async fn process_attachments(
        &self,
        attachments: &[serenity::model::channel::Attachment],
    ) -> String {
        if attachments.is_empty() {
            return String::new();
        }

        let download_dir = self.config.workspace.join("downloads");
        if let Err(e) = tokio::fs::create_dir_all(&download_dir).await {
            error!("Failed to create downloads dir: {e}");
            return String::new();
        }

        let timestamp = chrono::Utc::now().format("%s");
        let client = reqwest::Client::new();
        let mut lines = Vec::new();

        for attachment in attachments {
            let ext = attachment
                .filename
                .rsplit('.')
                .next()
                .unwrap_or("")
                .to_lowercase();

            let is_text = TEXT_EXTENSIONS.contains(&ext.as_str());

            if !is_text {
                lines.push(format!("[Attachment: {}]", attachment.filename));
                continue;
            }

            let dest_name = format!("{timestamp}_{}", attachment.filename);
            let dest_path = download_dir.join(&dest_name);

            match client.get(&attachment.url).send().await {
                Ok(resp) => match resp.bytes().await {
                    Ok(bytes) => {
                        if bytes.len() > MAX_ATTACHMENT_SIZE {
                            warn!(
                                filename = %attachment.filename,
                                size = bytes.len(),
                                "Attachment exceeds 25MB limit, skipping"
                            );
                            lines.push(format!("[Attachment too large: {}]", attachment.filename));
                            continue;
                        }
                        if let Err(e) = tokio::fs::write(&dest_path, &bytes).await {
                            error!("Failed to write attachment {dest_name}: {e}");
                            continue;
                        }
                        lines.push(format!("[Attachment downloaded: downloads/{dest_name}]"));
                    }
                    Err(e) => error!(
                        "Failed to download attachment body {}: {e}",
                        attachment.filename
                    ),
                },
                Err(e) => error!("Failed to fetch attachment {}: {e}", attachment.filename),
            }
        }

        if lines.is_empty() {
            return String::new();
        }
        lines.join("\n")
    }
}

#[async_trait]
impl EventHandler for Handler {
    #[tracing::instrument(skip_all, fields(
        author = %msg.author.name,
        channel_id = %msg.channel_id,
        content_len = msg.content.len()
    ))]
    async fn message(&self, ctx: Context, msg: Message) {
        // Ignore bots
        if msg.author.bot {
            return;
        }

        // Ignore non-allowed users (silent)
        if msg.author.id.to_string() != self.allowed_user_id {
            return;
        }

        // Skip messages from before this bot session (gateway resume replay)
        if *msg.timestamp < self.started_at {
            return;
        }

        let content = self.strip_bot_mention(&msg.content);

        // Handle /REBOOT command
        if content.eq_ignore_ascii_case("/reboot") {
            let session_id = match self.resolve_session(msg.channel_id).await {
                Ok(id) => id,
                Err(e) => {
                    error!("Failed to resolve session for reboot: {e}");
                    let _ = send_gateway_v2(
                        &ctx.http,
                        msg.channel_id,
                        "Failed to resolve session.",
                        Some(WARNING_EMBED_COLOR),
                    )
                    .await;
                    return;
                }
            };

            match self.session_chat.reboot_session(&session_id).await {
                Ok(new_id) => {
                    info!(
                        old_session = %session_id,
                        new_session = %new_id,
                        "session rebooted"
                    );
                    let _ = send_gateway_v2(
                        &ctx.http,
                        msg.channel_id,
                        "Session rebooted. Starting fresh.",
                        None,
                    )
                    .await;
                }
                Err(e) => {
                    error!("Reboot failed: {e}");
                    let _ = send_gateway_v2(
                        &ctx.http,
                        msg.channel_id,
                        &format!("Reboot failed: {e}"),
                        Some(WARNING_EMBED_COLOR),
                    )
                    .await;
                }
            }
            return;
        }

        // Process attachments
        let attachment_text = self.process_attachments(&msg.attachments).await;
        let full_content = if attachment_text.is_empty() {
            content.to_string()
        } else if content.is_empty() {
            attachment_text
        } else {
            format!("{content}\n\n{attachment_text}")
        };

        if full_content.trim().is_empty() {
            return;
        }

        // Resolve session
        let session_id = match self.resolve_session(msg.channel_id).await {
            Ok(id) => id,
            Err(e) => {
                error!("Failed to resolve session: {e}");
                let _ = send_gateway_v2(
                    &ctx.http,
                    msg.channel_id,
                    "Failed to create or find a session.",
                    Some(WARNING_EMBED_COLOR),
                )
                .await;
                return;
            }
        };

        // Start typing indicator
        let _typing = TimedTyping::start(msg.channel_id, &ctx.http);

        // Create event channel for live UI updates
        let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
        let renderer = DiscordUiRenderer::new(event_rx, Arc::clone(&ctx.http), msg.channel_id);
        let renderer_handle = tokio::spawn(renderer.run());

        // Chat with GHOST
        let chat_result = self
            .session_chat
            .chat(&session_id, &full_content, Some(&event_tx))
            .await;

        // Drop sender so renderer finishes
        drop(event_tx);
        let _ = renderer_handle.await;

        match chat_result {
            Ok((result, metadata)) => {
                let response_text = format_statusline(&result.message, &metadata);
                if let Err(e) = send_assistant_v2(&ctx.http, msg.channel_id, &response_text).await {
                    error!(
                        session_id = %session_id,
                        error = %e,
                        "Failed to send assistant response"
                    );
                }

                if result.stop_reason == ChatStopReason::MaxIterations {
                    let _ = send_gateway_v2(
                        &ctx.http,
                        msg.channel_id,
                        "Reached tool iteration limit. Send another \
                         message to continue.",
                        Some(WARNING_EMBED_COLOR),
                    )
                    .await;
                }
            }
            Err(e) => {
                error!(
                    session_id = %session_id,
                    error = %e,
                    "Chat error"
                );
                let _ = send_gateway_v2(
                    &ctx.http,
                    msg.channel_id,
                    &format!("Error: {e}"),
                    Some(WARNING_EMBED_COLOR),
                )
                .await;
            }
        }
    }

    async fn ready(&self, _ctx: Context, ready: Ready) {
        info!(
            bot_name = %ready.user.name,
            bot_id = %ready.user.id,
            "Discord bot connected"
        );
        let _ = self.bot_user_id.set(ready.user.id.to_string());
    }
}
