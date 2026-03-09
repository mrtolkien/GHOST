use std::sync::Arc;

use serenity::http::Http;
use serenity::model::id::ChannelId;
use serenity::prelude::*;
use tokio::task::JoinHandle;
use tracing::info;

use crate::chat::{ActiveSessions, SessionChat};
use crate::config::Config;
use crate::db::GhostDb;

use super::components_v2::{container, send_v2_message, text_display};
use super::send::{GATEWAY_EMBED_COLOR, send_assistant_v2, send_gateway_v2};
use super::table_image;

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
    pub fn http(&self) -> &Arc<Http> {
        &self.http
    }

    /// Send GHOST assistant-style content to a channel.
    #[tracing::instrument(skip_all, fields(channel_id = %channel_id))]
    pub async fn send_to_channel(&self, channel_id: u64, content: &str) -> serenity::Result<()> {
        send_assistant_v2(&self.http, ChannelId::new(channel_id), content).await
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

/// Start the Discord bot if configured. Returns `None` if Discord is
/// disabled or unconfigured.
#[tracing::instrument(skip_all)]
pub async fn start_discord(
    config: &Config,
    session_chat: Arc<SessionChat>,
    db: GhostDb,
    active_sessions: ActiveSessions,
    bundled_update_tx: Option<tokio::sync::mpsc::UnboundedSender<String>>,
) -> Result<Option<(DiscordSender, JoinHandle<()>)>, DiscordError> {
    if !config.discord.enabled {
        info!("Discord is disabled in config");
        return Ok(None);
    }

    if config.discord.allowed_user_ids.is_empty() {
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

    let handler = super::bot::Handler::new(
        session_chat,
        db,
        config.clone(),
        config.discord.allowed_user_ids.clone(),
        active_sessions,
        bundled_update_tx,
    );

    let mut client = Client::builder(&token, intents)
        .event_handler(handler)
        .await
        .map_err(|e| DiscordError::ClientBuild(e.to_string()))?;

    let sender = DiscordSender {
        http: client.http.clone(),
    };

    let handle = tokio::spawn(async move {
        if let Err(e) = client.start().await {
            tracing::error!("Discord client error: {e}");
        }
    });

    Ok(Some((sender, handle)))
}
