use std::path::PathBuf;

use serenity::builder::{CreateAttachment, CreateMessage};
use serenity::http::Http;
use serenity::model::id::ChannelId;

use crate::error::GhostError;

/// Send a file (image or generic) to the OPERATOR via Discord.
async fn send_file(
    path: &std::path::Path,
    caption: Option<&str>,
    is_image: bool,
) -> Result<(), GhostError> {
    let channel_id: u64 = std::env::var("GHOST_CHANNEL_ID")
        .map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "GHOST_CHANNEL_ID not set — run this from a GHOST shell session",
            )
        })?
        .parse()
        .map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "GHOST_CHANNEL_ID is not a valid u64",
            )
        })?;

    let session_id = std::env::var("GHOST_SESSION_ID").ok();

    if !path.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("file not found: {}", path.display()),
        )
        .into());
    }

    let file_data = std::fs::read(path)?;
    let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");

    // Send via Discord
    let token = std::env::var("DISCORD_BOT_TOKEN").map_err(|_| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "DISCORD_BOT_TOKEN not set")
    })?;

    let http = Http::new(&token);
    let channel = ChannelId::new(channel_id);

    let attachment = CreateAttachment::bytes(file_data, filename);
    let mut message = CreateMessage::new().add_file(attachment);
    if let Some(cap) = caption {
        message = message.content(cap);
    }

    channel
        .send_message(&http, message)
        .await
        .map_err(|e| std::io::Error::other(e.to_string()))?;

    // Record in session history (best-effort)
    if let Some(ref sid) = session_id {
        if let Ok(config) = crate::config::load() {
            let _ = crate::config_workspace::bootstrap_workspace(&config);
            if let Ok(db) = crate::db::connect(&config.workspace, config.embeddings.dimension).await
            {
                let kind = if is_image { "image" } else { "file" };
                let cap_suffix = caption.map(|c| format!(" — {c}")).unwrap_or_default();
                let msg = format!("[sent {kind}: {filename}{cap_suffix}]");
                let _ = crate::db::sessions::create_message(&db, sid, "system", &msg).await;
            }
        }
    }

    let kind = if is_image { "Image" } else { "File" };
    println!("{kind} sent: {filename}");
    Ok(())
}

pub async fn execute_send_image(path: PathBuf, caption: Option<String>) -> Result<(), GhostError> {
    send_file(&path, caption.as_deref(), true).await
}

pub async fn execute_attach(path: PathBuf, caption: Option<String>) -> Result<(), GhostError> {
    send_file(&path, caption.as_deref(), false).await
}
