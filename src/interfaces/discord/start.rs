use std::sync::Arc;

use serenity::gateway::ShardManager;
use serenity::http::Http;
use serenity::model::id::ChannelId;
use serenity::prelude::*;
use tokio::task::JoinHandle;
use tracing::info;

use crate::chat::{ActiveSessions, SessionChat};
use crate::config::{SharedConfig, SharedConfigExt};
use crate::db::GhostDb;

use super::components_v2::{action_row, button, container, send_v2_message, text_display};
use super::send::{
    GATEWAY_EMBED_COLOR, send_assistant_v2, send_assistant_v2_with_suffix, send_gateway_v2,
};
use super::table_image;
use crate::tools::confirmation::{ConfirmationReceiver, OptionStyle};

#[derive(Debug, thiserror::Error)]
pub enum DiscordError {
    #[error("failed to create Discord client: {0}")]
    ClientBuild(String),

    #[error("Discord connection error: {0}")]
    Connection(String),

    #[error("DISCORD_BOT_TOKEN environment variable is not set")]
    MissingToken,

    #[error("discord.allowed_user_id is not configured")]
    MissingAllowedUser,
}

/// Handle for sending messages to Discord channels from outside the event
/// handler (e.g. reflection jobs).
#[derive(Debug, Clone)]
pub struct DiscordSender {
    http: Arc<Http>,
}

impl DiscordSender {
    /// Create a sender from a raw bot token (for tests / CLI).
    pub fn from_token(token: &str) -> Self {
        Self {
            http: Arc::new(Http::new(token)),
        }
    }

    pub fn http(&self) -> &Arc<Http> {
        &self.http
    }

    /// Send GHOST assistant-style content to a channel.
    #[tracing::instrument(skip_all, fields(channel_id = %channel_id))]
    pub async fn send_to_channel(&self, channel_id: u64, content: &str) -> serenity::Result<()> {
        send_assistant_v2(&self.http, ChannelId::new(channel_id), content).await
    }

    /// Send GHOST assistant-style content with statusline suffix components.
    #[tracing::instrument(skip_all, fields(channel_id = %channel_id))]
    pub async fn send_to_channel_with_suffix(
        &self,
        channel_id: u64,
        content: &str,
        suffix: &[serde_json::Value],
    ) -> serenity::Result<()> {
        send_assistant_v2_with_suffix(&self.http, ChannelId::new(channel_id), content, suffix).await
    }

    /// Send a system/gateway message to a channel.
    #[tracing::instrument(skip_all, fields(channel_id = %channel_id))]
    pub async fn send_system_message(
        &self,
        channel_id: u64,
        content: &str,
        color: Option<u32>,
    ) -> serenity::Result<()> {
        send_gateway_v2(&self.http, ChannelId::new(channel_id), content, color).await
    }

    /// Send a compact v2 container without the "GHOST" header.
    #[tracing::instrument(skip_all, fields(channel_id = %channel_id))]
    pub async fn send_compact_container(
        &self,
        channel_id: u64,
        content: &str,
        color: Option<u32>,
    ) -> serenity::Result<()> {
        let accent = color.unwrap_or(GATEWAY_EMBED_COLOR);
        let components = vec![container(vec![text_display(content)], Some(accent))];
        send_v2_message(
            &self.http,
            ChannelId::new(channel_id),
            &components,
            Vec::new(),
        )
        .await
        .map(|_| ())
    }
}

/// Running Discord connection — sender, shard manager, and background task.
pub struct DiscordHandle {
    pub sender: DiscordSender,
    shard_manager: Arc<ShardManager>,
    handle: JoinHandle<()>,
}

impl DiscordHandle {
    /// Gracefully close all shards and wait for the client task to exit.
    pub async fn shutdown(self) {
        self.shard_manager.shutdown_all().await;
        let _ = self.handle.await;
    }
}

/// Peach accent color for confirmation containers.
const CONFIRMATION_ACCENT: u32 = 0xFA_B3_87;

/// Start the Discord bot if configured. Returns `None` if Discord is
/// disabled or unconfigured.
#[tracing::instrument(skip_all)]
pub async fn start_discord(
    config: &SharedConfig,
    session_chat: Arc<SessionChat>,
    db: GhostDb,
    active_sessions: ActiveSessions,
    bundled_update_tx: Option<tokio::sync::mpsc::UnboundedSender<String>>,
    confirmation_rx: Option<ConfirmationReceiver>,
) -> Result<Option<DiscordHandle>, DiscordError> {
    let cfg = config.current();
    if !cfg.discord.enabled {
        info!("Discord is disabled in config");
        return Ok(None);
    }

    if cfg.discord.allowed_user_ids.is_empty() {
        return Err(DiscordError::MissingAllowedUser);
    }

    let token = match std::env::var("DISCORD_BOT_TOKEN") {
        Ok(t) if !t.is_empty() => t,
        _ => {
            info!("DISCORD_BOT_TOKEN not set, skipping Discord");
            return Ok(None);
        }
    };

    info!("Starting Discord bot...");

    // Eagerly load system fonts so the first table-to-PNG render doesn't
    // block the tokio runtime.
    table_image::init_fonts();
    info!("System font database initialized");

    let intents = GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::DIRECT_MESSAGES
        | GatewayIntents::MESSAGE_CONTENT;

    let pending_confirmations = Arc::new(dashmap::DashMap::new());

    let handler = super::bot::Handler::new(
        session_chat,
        db,
        config.clone(),
        active_sessions,
        bundled_update_tx,
        Arc::clone(&pending_confirmations),
    );

    let mut client = Client::builder(&token, intents)
        .event_handler(handler)
        .await
        .map_err(|e| DiscordError::ClientBuild(e.to_string()))?;

    let sender = DiscordSender {
        http: client.http.clone(),
    };
    let shard_manager = client.shard_manager.clone();

    // Spawn confirmation renderer (reads ConfirmationRequests, sends v2 messages)
    if let Some(confirmation_rx) = confirmation_rx {
        spawn_confirmation_renderer(client.http.clone(), pending_confirmations, confirmation_rx);
    }

    let handle = tokio::spawn(async move {
        if let Err(e) = client.start().await {
            tracing::error!("Discord client error: {e}");
        }
    });

    Ok(Some(DiscordHandle {
        sender,
        shard_manager,
        handle,
    }))
}

/// Spawn a background task that receives `ConfirmationRequest`s and renders
/// them as v2 button messages in the appropriate Discord channel.
fn spawn_confirmation_renderer(
    http: Arc<Http>,
    pending: Arc<dashmap::DashMap<String, tokio::sync::oneshot::Sender<String>>>,
    mut rx: ConfirmationReceiver,
) {
    tokio::spawn(async move {
        while let Some(req) = rx.recv().await {
            let Some(channel_str) = req.channel_id.as_deref() else {
                tracing::warn!("confirmation request has no channel_id, dropping");
                continue;
            };
            let Ok(channel_num) = channel_str.parse::<u64>() else {
                tracing::warn!(
                    channel_id = channel_str,
                    "invalid channel_id in confirmation request"
                );
                continue;
            };
            let channel_id = ChannelId::new(channel_num);

            let uuid = ulid::Ulid::new().to_string().to_lowercase();

            // Store the response sender so interaction_create can resolve it
            pending.insert(uuid.clone(), req.response_tx);

            // Build display text
            let mut parts = vec![req.confirmation.prompt.clone()];
            if let Some(ctx) = &req.confirmation.context {
                parts.push(format!("```diff\n{ctx}\n```"));
            }
            let display = parts.join("\n");

            let buttons: Vec<serde_json::Value> = req
                .confirmation
                .options
                .iter()
                .map(|opt| {
                    let style = match opt.style {
                        OptionStyle::Primary => 1u8,
                        OptionStyle::Secondary => 2,
                        OptionStyle::Danger => 4,
                    };
                    button(&opt.label, &format!("confirm_{}_{}", opt.id, uuid), style)
                })
                .collect();

            let components = vec![container(
                vec![text_display(&display), action_row(buttons)],
                Some(CONFIRMATION_ACCENT),
            )];

            if let Err(e) = send_v2_message(&http, channel_id, &components, Vec::new()).await {
                tracing::error!(
                    channel_id = %channel_id,
                    "failed to send confirmation message: {e}"
                );
            }
        }
    });
}
