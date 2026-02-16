mod bot;
mod components_v2;
mod markdown;
pub(crate) mod send;
mod table_image;

use std::sync::Arc;

use serenity::http::Http;
use serenity::model::id::ChannelId;
use serenity::prelude::*;
use surrealdb::Surreal;
use surrealdb::engine::local::Db;
use tokio::task::JoinHandle;
use tracing::info;

use crate::chat::SessionChat;
use crate::config::Config;

use send::{send_assistant_v2, send_gateway_v2};

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
/// handler (e.g. heartbeat/reflection jobs).
#[derive(Debug, Clone)]
pub struct DiscordSender {
    http: Arc<Http>,
}

impl DiscordSender {
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
}

/// Start the Discord bot if configured. Returns `None` if Discord is
/// disabled or unconfigured.
#[tracing::instrument(skip_all)]
pub async fn start_discord(
    config: &Config,
    session_chat: Arc<SessionChat>,
    db: Surreal<Db>,
) -> Result<Option<(DiscordSender, JoinHandle<()>)>, DiscordError> {
    if !config.discord.enabled {
        info!("Discord is disabled in config");
        return Ok(None);
    }

    if config.discord.allowed_user_id.is_empty() {
        return Err(DiscordError::MissingAllowedUser);
    }

    let token = std::env::var("DISCORD_BOT_TOKEN").map_err(|_| DiscordError::MissingToken)?;

    if token.is_empty() {
        return Err(DiscordError::MissingToken);
    }

    info!("Starting Discord bot...");

    // Eagerly load system fonts so the first table-to-PNG render doesn't
    // block the tokio runtime.
    table_image::init_fonts();
    info!("System font database initialized");

    let intents = GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::DIRECT_MESSAGES
        | GatewayIntents::MESSAGE_CONTENT;

    let handler = bot::Handler::new(
        session_chat,
        db,
        config.clone(),
        config.discord.allowed_user_id.clone(),
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
