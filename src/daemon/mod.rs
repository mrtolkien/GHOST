use std::sync::Arc;

use tracing::info;

use crate::chat::SessionChat;
use crate::error::GhostError;
use crate::interfaces::discord;

pub async fn run() -> Result<(), GhostError> {
    info!("loading config");
    let config = crate::config::load()?;
    crate::config::bootstrap_workspace(&config)?;
    info!(workspace = %config.workspace.display(), "config loaded");
    info!("connecting to database");
    let db = crate::db::connect(&config.workspace).await?;
    info!("database ready");

    let session_chat = Arc::new(SessionChat::from_config(db.clone(), config.clone())?);

    let discord_result = discord::start_discord(&config, session_chat, db).await?;

    if let Some((_sender, handle)) = discord_result {
        info!("GHOST daemon running — press Ctrl+C to stop");

        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                info!("Ctrl+C received, shutting down...");
            }
            _ = handle => {
                info!("Discord bot task ended");
            }
        }
    } else {
        info!("No interfaces enabled. Nothing to run.");
    }

    info!("GHOST daemon stopped");
    Ok(())
}
