use std::sync::{Arc, OnceLock};
use std::time::Duration;

use serenity::async_trait;
use serenity::builder::{
    CreateCommand, CreateInteractionResponse, CreateInteractionResponseMessage,
};
use serenity::http::Http;
use serenity::model::application::Command;
use serenity::model::channel::Message;
use serenity::model::gateway::Ready;
use serenity::model::id::ChannelId;
use serenity::prelude::*;
use tokio::task::JoinHandle;
use tracing::{Instrument, error, info, warn};

use crate::chat::{ActiveSessions, ChatStopReason, SessionChat};
use crate::coding;
use crate::config::Config;
use crate::db;
use crate::db::GhostDb;
use crate::providers::ContentBlock;

use super::feedback;
use super::send::{WARNING_EMBED_COLOR, send_assistant_v2_with_suffix, send_gateway_v2};

/// Teal embed color for coding session messages.
const CODING_EMBED_COLOR: u32 = 0x29_FF_D9;
use super::ui_events::DiscordUiRenderer;
use super::ui_events::format_statusline;

/// Maximum duration for a typing indicator before it auto-stops.
const TYPING_TIMEOUT: Duration = Duration::from_secs(300);

/// Max file size for attachment downloads (25 MB).
const MAX_ATTACHMENT_SIZE: usize = 25 * 1024 * 1024;

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
    db: GhostDb,
    config: Config,
    allowed_user_ids: Vec<String>,
    bot_user_id: OnceLock<String>,
    started_at: std::time::SystemTime,
    active_sessions: ActiveSessions,
    /// Channel for bundled file update button responses.
    bundled_update_tx: Option<tokio::sync::mpsc::UnboundedSender<String>>,
}

impl Handler {
    pub fn new(
        session_chat: Arc<SessionChat>,
        db: GhostDb,
        config: Config,
        allowed_user_ids: Vec<String>,
        active_sessions: ActiveSessions,
        bundled_update_tx: Option<tokio::sync::mpsc::UnboundedSender<String>>,
    ) -> Self {
        Self {
            session_chat,
            db,
            config,
            allowed_user_ids,
            bot_user_id: OnceLock::new(),
            started_at: std::time::SystemTime::now(),
            active_sessions,
            bundled_update_tx,
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
            return Ok(thing);
        }

        let new_session = db::sessions::create_session(&self.db).await?;
        db::interface_sessions::set_active_session_for_interface(&self.db, &iface, &new_session)
            .await?;
        let new_session_str = new_session;
        info!(
            session_id = %new_session_str,
            channel_id = %channel_id,
            "created new session for channel"
        );
        Ok(new_session_str)
    }

    /// Download attachments to workspace/uploads/. Returns content blocks:
    /// image attachments become `ContentBlock::Image`, others become text.
    #[tracing::instrument(skip_all, level = "debug", fields(
        attachment_count = attachments.len()
    ))]
    async fn process_attachments(
        &self,
        attachments: &[serenity::model::channel::Attachment],
    ) -> Vec<ContentBlock> {
        if attachments.is_empty() {
            return Vec::new();
        }

        let upload_dir = self.config.workspace.join("uploads");
        if let Err(e) = tokio::fs::create_dir_all(&upload_dir).await {
            error!("Failed to create uploads dir: {e}");
            return Vec::new();
        }

        let timestamp = chrono::Utc::now().format("%s");
        let client = reqwest::Client::new();
        let mut blocks = Vec::new();

        for attachment in attachments {
            let ext = attachment
                .filename
                .rsplit('.')
                .next()
                .unwrap_or("")
                .to_lowercase();

            let dest_name = format!("{timestamp}_{}", attachment.filename);
            let dest_path = upload_dir.join(&dest_name);

            match client.get(&attachment.url).send().await {
                Ok(resp) => match resp.bytes().await {
                    Ok(bytes) => {
                        if bytes.len() > MAX_ATTACHMENT_SIZE {
                            warn!(
                                filename = %attachment.filename,
                                size = bytes.len(),
                                "Attachment exceeds 25MB limit, skipping"
                            );
                            blocks.push(ContentBlock::Text {
                                text: format!("[Attachment too large: {}]", attachment.filename),
                            });
                            continue;
                        }
                        if let Err(e) = tokio::fs::write(&dest_path, &bytes).await {
                            error!("Failed to write attachment {dest_name}: {e}");
                            continue;
                        }

                        if crate::images::is_image_extension(&ext) {
                            let mime = crate::images::mime_type_from_extension(&ext).to_string();
                            blocks.push(ContentBlock::Image {
                                path: dest_path.to_string_lossy().to_string(),
                                mime_type: mime,
                                filename: attachment.filename.clone(),
                            });
                        } else {
                            blocks.push(ContentBlock::Text {
                                text: format!("[File uploaded: uploads/{dest_name}]"),
                            });
                        }
                    }
                    Err(e) => error!(
                        "Failed to download attachment body {}: {e}",
                        attachment.filename
                    ),
                },
                Err(e) => error!("Failed to fetch attachment {}: {e}", attachment.filename),
            }
        }

        blocks
    }
    /// Handle a validated incoming Discord message: resolve the session,
    /// process attachments, run the chat loop, and send the response.
    #[tracing::instrument(name = "receive discord message", skip_all, fields(
        author = %msg.author.name,
        channel_id = %msg.channel_id,
        content_len = msg.content.len()
    ))]
    async fn handle_message(&self, ctx: Context, msg: Message) {
        let content = self.strip_bot_mention(&msg.content);

        // Check for active coding session takeover
        let channel_str = msg.channel_id.to_string();
        if let Ok(Some((_coding_id, session_id, working_dir))) =
            db::coding_sessions::get_active_takeover(&self.db, &channel_str).await
        {
            // Send entry banner on first interaction
            if let Ok(count) = db::sessions::count_messages_for_session(&self.db, &session_id).await
                && count <= 1
            {
                let _ = send_gateway_v2(
                    &ctx.http,
                    msg.channel_id,
                    "**GHOST HACKED** — you're now talking to the coding agent. \
                     Send `/kill` to end the session.",
                    Some(CODING_EMBED_COLOR),
                )
                .await;
            }

            self.handle_coding_message(ctx, msg, &session_id, &working_dir)
                .await;
            return;
        }

        // Process attachments
        let attachment_blocks = self.process_attachments(&msg.attachments).await;

        // Separate image blocks from text blocks
        let mut text_parts = Vec::new();
        let mut image_blocks = Vec::new();
        if !content.is_empty() {
            text_parts.push(content.to_string());
        }
        for block in attachment_blocks {
            match block {
                ContentBlock::Text { text } => text_parts.push(text),
                img @ ContentBlock::Image { .. } => image_blocks.push(img),
                _ => {}
            }
        }
        let full_content = text_parts.join("\n\n");

        if full_content.trim().is_empty() && image_blocks.is_empty() {
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

        // If a tool loop is already running for this session, steer it
        if let Some(tx) = self.active_sessions.get(&session_id) {
            let _ = tx.send(crate::chat::interrupt::Interrupt::Steer {
                message: full_content,
            });
            return;
        }

        // Start typing indicator
        let _typing = TimedTyping::start(msg.channel_id, &ctx.http);

        // Create event channel for live UI updates
        let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
        let renderer = DiscordUiRenderer::new(event_rx, Arc::clone(&ctx.http), msg.channel_id);
        let renderer_handle = tokio::spawn(renderer.run());

        // Chat with GHOST
        let images = if image_blocks.is_empty() {
            None
        } else {
            Some(image_blocks)
        };
        let chat_result = self
            .session_chat
            .chat_with_images(
                &session_id,
                &full_content,
                images,
                Some(msg.channel_id.to_string()),
                Some(&event_tx),
            )
            .await;

        // Drop sender so renderer finishes
        drop(event_tx);
        let _ = renderer_handle.await;

        match chat_result {
            Ok((result, metadata)) => {
                let statusline = format_statusline(&metadata);
                if let Err(e) = send_assistant_v2_with_suffix(
                    &ctx.http,
                    msg.channel_id,
                    &result.message,
                    &statusline,
                )
                .await
                {
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

                if result.stop_reason == ChatStopReason::Stopped {
                    let _ = send_gateway_v2(&ctx.http, msg.channel_id, "Stopped.", None).await;
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

    /// Handle a message routed to the coding agent session.
    #[tracing::instrument(name = "receive coding message", skip_all, fields(
        session_id = session_id,
        working_dir = working_dir,
    ))]
    async fn handle_coding_message(
        &self,
        ctx: Context,
        msg: Message,
        session_id: &str,
        working_dir: &str,
    ) {
        let content = self.strip_bot_mention(&msg.content);

        // Process attachments (coding sessions: images treated as text refs)
        let attachment_blocks = self.process_attachments(&msg.attachments).await;
        let mut text_parts = Vec::new();
        if !content.is_empty() {
            text_parts.push(content.to_string());
        }
        for block in attachment_blocks {
            match block {
                ContentBlock::Text { text } => text_parts.push(text),
                ContentBlock::Image { filename, .. } => {
                    text_parts.push(format!("[Image uploaded: {filename}]"));
                }
                _ => {}
            }
        }
        let full_content = text_parts.join("\n\n");

        if full_content.trim().is_empty() {
            return;
        }

        // If a tool loop is already running for this coding session, steer it
        if let Some(tx) = self.active_sessions.get(session_id) {
            let _ = tx.send(crate::chat::interrupt::Interrupt::Steer {
                message: full_content,
            });
            return;
        }

        let _typing = TimedTyping::start(msg.channel_id, &ctx.http);

        let working_path = std::path::Path::new(working_dir);
        let system_prompt = coding::prompt::build_coding_prompt(&self.config, working_path);

        let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
        let renderer = DiscordUiRenderer::new(event_rx, Arc::clone(&ctx.http), msg.channel_id);
        let renderer_handle = tokio::spawn(renderer.run());

        let chat_result = self
            .session_chat
            .chat_coding(
                session_id,
                &full_content,
                &system_prompt,
                working_path,
                Some(msg.channel_id.to_string()),
                Some(&event_tx),
            )
            .await;

        drop(event_tx);
        let _ = renderer_handle.await;

        match chat_result {
            Ok((result, metadata)) => {
                let statusline = format_statusline(&metadata);
                if let Err(e) = send_assistant_v2_with_suffix(
                    &ctx.http,
                    msg.channel_id,
                    &result.message,
                    &statusline,
                )
                .await
                {
                    error!(
                        session_id = %session_id,
                        error = %e,
                        "Failed to send coding response"
                    );
                }

                if result.stop_reason == ChatStopReason::MaxIterations {
                    let _ = send_gateway_v2(
                        &ctx.http,
                        msg.channel_id,
                        "Reached tool iteration limit. Send another message to continue.",
                        Some(WARNING_EMBED_COLOR),
                    )
                    .await;
                }

                if result.stop_reason == ChatStopReason::Stopped {
                    let _ = send_gateway_v2(&ctx.http, msg.channel_id, "Stopped.", None).await;
                }
            }
            Err(e) => {
                error!(
                    session_id = %session_id,
                    error = %e,
                    "Coding chat error"
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
}

#[async_trait]
impl EventHandler for Handler {
    async fn message(&self, ctx: Context, msg: Message) {
        // Quick-reject before creating a span
        if msg.author.bot {
            return;
        }
        let author_id = msg.author.id.to_string();
        if !self.allowed_user_ids.iter().any(|id| id == &author_id) {
            return;
        }
        if *msg.timestamp < self.started_at {
            return;
        }
        self.handle_message(ctx, msg).await;
    }

    async fn interaction_create(
        &self,
        ctx: Context,
        interaction: serenity::model::application::Interaction,
    ) {
        // Handle component interactions (button clicks)
        if let Some(component) = interaction.as_message_component() {
            let custom_id = component.data.custom_id.clone();

            async {
                if let Err(e) = component
                    .create_response(&ctx.http, CreateInteractionResponse::Acknowledge)
                    .await
                {
                    error!(
                        custom_id,
                        "failed to acknowledge component interaction: {e}"
                    );
                }

                // Forward bundled update responses
                if custom_id.starts_with("bundled_") {
                    if let Some(tx) = self.bundled_update_tx.as_ref() {
                        let _ = tx.send(custom_id);
                    } else {
                        warn!("bundled interaction received but no handler");
                    }
                }
            }
            .instrument(tracing::info_span!("handle component interaction"))
            .await;

            return;
        }

        let Some(command) = interaction.as_command() else {
            return;
        };

        let channel_id = command.channel_id;

        match command.data.name.as_str() {
            "stop" => {
                let session_id = match self.resolve_session(channel_id).await {
                    Ok(id) => id,
                    Err(e) => {
                        error!("Failed to resolve session for /stop: {e}");
                        let _ = command
                            .create_response(
                                &ctx.http,
                                CreateInteractionResponse::Message(
                                    CreateInteractionResponseMessage::new()
                                        .content("Failed to resolve session.")
                                        .ephemeral(true),
                                ),
                            )
                            .await;
                        return;
                    }
                };

                if let Some(tx) = self.active_sessions.get(&session_id) {
                    let _ = tx.send(crate::chat::interrupt::Interrupt::Stop);
                    let _ = command
                        .create_response(
                            &ctx.http,
                            CreateInteractionResponse::Message(
                                CreateInteractionResponseMessage::new()
                                    .content("Stopping after current operation finishes."),
                            ),
                        )
                        .await;
                } else {
                    let _ = command
                        .create_response(
                            &ctx.http,
                            CreateInteractionResponse::Message(
                                CreateInteractionResponseMessage::new()
                                    .content("Nothing is running right now.")
                                    .ephemeral(true),
                            ),
                        )
                        .await;
                }
            }
            "reboot" => {
                let session_id = match self.resolve_session(channel_id).await {
                    Ok(id) => id,
                    Err(e) => {
                        error!("Failed to resolve session for /reboot: {e}");
                        let _ = command
                            .create_response(
                                &ctx.http,
                                CreateInteractionResponse::Message(
                                    CreateInteractionResponseMessage::new()
                                        .content("Failed to resolve session.")
                                        .ephemeral(true),
                                ),
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
                        let _ = command
                            .create_response(
                                &ctx.http,
                                CreateInteractionResponse::Message(
                                    CreateInteractionResponseMessage::new()
                                        .content("Session rebooted. Starting fresh."),
                                ),
                            )
                            .await;
                    }
                    Err(e) => {
                        error!("Reboot failed: {e}");
                        let _ = command
                            .create_response(
                                &ctx.http,
                                CreateInteractionResponse::Message(
                                    CreateInteractionResponseMessage::new()
                                        .content(format!("Reboot failed: {e}"))
                                        .ephemeral(true),
                                ),
                            )
                            .await;
                    }
                }
            }
            "kill" => {
                let channel_str = channel_id.to_string();
                match db::coding_sessions::get_active_takeover(&self.db, &channel_str).await {
                    Ok(Some((coding_id, _session_id, working_dir))) => {
                        let summary = coding::session::end(
                            &self.db,
                            &coding_id,
                            std::path::Path::new(&working_dir),
                        )
                        .await
                        .unwrap_or_else(|e| format!("(summary failed: {e})"));

                        // Inject summary into GHOST's main session
                        if let Ok(ghost_sid) = self.resolve_session(channel_id).await {
                            let summary_msg = format!("[coding session ended]\n\n{summary}");
                            let _ = db::sessions::create_message(
                                &self.db,
                                &ghost_sid,
                                "system",
                                &summary_msg,
                            )
                            .await;
                        }

                        let _ = command
                            .create_response(
                                &ctx.http,
                                CreateInteractionResponse::Message(
                                    CreateInteractionResponseMessage::new().content(format!(
                                        "GHOST HACKED -- session ended.\n\n```\n{summary}\n```"
                                    )),
                                ),
                            )
                            .await;
                    }
                    Ok(None) => {
                        let _ = command
                            .create_response(
                                &ctx.http,
                                CreateInteractionResponse::Message(
                                    CreateInteractionResponseMessage::new()
                                        .content("No active coding session.")
                                        .ephemeral(true),
                                ),
                            )
                            .await;
                    }
                    Err(e) => {
                        error!("Failed to check takeover: {e}");
                        let _ = command
                            .create_response(
                                &ctx.http,
                                CreateInteractionResponse::Message(
                                    CreateInteractionResponseMessage::new()
                                        .content("Internal error checking coding session.")
                                        .ephemeral(true),
                                ),
                            )
                            .await;
                    }
                }
            }
            "feedback" => {
                let feedback_message = command
                    .data
                    .options
                    .iter()
                    .find(|o| o.name == "message")
                    .and_then(|o| o.value.as_str())
                    .unwrap_or("(no message)")
                    .to_string();

                let session_id = match self.resolve_session(channel_id).await {
                    Ok(id) => id,
                    Err(e) => {
                        error!("Failed to resolve session for /feedback: {e}");
                        let _ = command
                            .create_response(
                                &ctx.http,
                                CreateInteractionResponse::Message(
                                    CreateInteractionResponseMessage::new()
                                        .content("Failed to resolve session.")
                                        .ephemeral(true),
                                ),
                            )
                            .await;
                        return;
                    }
                };

                let timestamp = chrono::Utc::now().format("%Y-%m-%dT%H-%M-%S").to_string();
                let slug = feedback::make_slug(&feedback_message);
                let folder_name = format!("{timestamp}-{slug}");
                let feedback_dir = self.config.workspace.join("feedback").join(&folder_name);

                match feedback::save_feedback(
                    &self.config.workspace,
                    &feedback_dir,
                    &self.db,
                    &session_id,
                    &feedback_message,
                )
                .await
                {
                    Ok(name) => {
                        info!(folder = %name, session_id = %session_id, "feedback saved");
                        let _ = command
                            .create_response(
                                &ctx.http,
                                CreateInteractionResponse::Message(
                                    CreateInteractionResponseMessage::new()
                                        .content(format!("Feedback saved to `feedback/{name}/`"))
                                        .ephemeral(true),
                                ),
                            )
                            .await;
                    }
                    Err(e) => {
                        error!("Failed to save feedback: {e}");
                        let _ = command
                            .create_response(
                                &ctx.http,
                                CreateInteractionResponse::Message(
                                    CreateInteractionResponseMessage::new()
                                        .content(format!("Failed to save feedback: {e}"))
                                        .ephemeral(true),
                                ),
                            )
                            .await;
                    }
                }
            }
            _ => {}
        }
    }

    async fn ready(&self, ctx: Context, ready: Ready) {
        info!(
            bot_name = %ready.user.name,
            bot_id = %ready.user.id,
            "Discord bot connected"
        );
        let _ = self.bot_user_id.set(ready.user.id.to_string());

        let commands = vec![
            CreateCommand::new("stop").description("Stop the current operation"),
            CreateCommand::new("reboot").description("Start a fresh session"),
            CreateCommand::new("kill").description("End the active coding session"),
            CreateCommand::new("feedback")
                .description("Report an issue with the last interaction")
                .add_option(
                    serenity::builder::CreateCommandOption::new(
                        serenity::model::application::CommandOptionType::String,
                        "message",
                        "What went wrong?",
                    )
                    .required(true),
                ),
        ];

        if let Err(e) = Command::set_global_commands(&ctx.http, commands).await {
            error!("Failed to register slash commands: {e}");
        }
    }
}
