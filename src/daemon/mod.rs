pub mod watcher;

use std::sync::Arc;

use tracing::info;

use crate::chat::SessionChat;
use crate::embeddings::EmbeddingClient;
use crate::error::GhostError;
use crate::interfaces::discord;
use crate::jobs::{HeartbeatManager, ReflectionManager};

pub async fn run() -> Result<(), GhostError> {
    info!("loading config");
    let config = crate::config::load()?;
    crate::config::bootstrap_workspace(&config)?;
    info!(workspace = %config.workspace.display(), "config loaded");
    info!("connecting to database");
    let db = crate::db::connect(&config.workspace).await?;
    info!("database ready");

    // Boot reconciliation: embed any knowledge that is missing or outdated
    let client = EmbeddingClient::new(&config.embeddings);
    if client.is_available().await {
        info!("running embedding boot reconciliation");
        match crate::embeddings::pipeline::reconcile_embeddings(&client, &db).await {
            Ok((embedded, skipped)) => {
                info!(embedded, skipped, "boot reconciliation complete");
            }
            Err(e) => {
                logfire::warn!("boot reconciliation failed", error = e.to_string(),);
            }
        }
    } else {
        logfire::warn!("Ollama unavailable — skipping boot reconciliation");
    }

    // Spawn file watcher for automatic embedding on content changes
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let watcher_handle = watcher::spawn_watcher(
        db.clone(),
        config.workspace.clone(),
        config.embeddings.clone(),
        shutdown_rx.clone(),
    );

    // Spawn the job scheduler
    let scheduler_handle =
        crate::jobs::spawn_scheduler(db.clone(), config.clone(), shutdown_rx.clone());

    let session_chat = Arc::new(SessionChat::from_config(db.clone(), config.clone())?);

    let discord_result = discord::start_discord(&config, session_chat.clone(), db.clone()).await?;

    // Spawn heartbeat manager (only if Discord is available for sending)
    let heartbeat_handle = if let Some((ref sender, _)) = discord_result {
        let reflection = Arc::new(ReflectionManager::new(db.clone(), config.clone()));
        let hb = HeartbeatManager::new(
            db.clone(),
            session_chat,
            Arc::new(sender.clone()),
            config.clone(),
            reflection,
        );
        Some(hb.spawn(shutdown_rx))
    } else {
        None
    };

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
        info!("No interfaces enabled. Waiting for Ctrl+C...");
        let _ = tokio::signal::ctrl_c().await;
    }

    // Signal shutdown to watcher, scheduler, and heartbeat
    let _ = shutdown_tx.send(true);
    let _ = watcher_handle.await;
    let _ = scheduler_handle.await;
    if let Some(hb_handle) = heartbeat_handle {
        let _ = hb_handle.await;
    }

    info!("GHOST daemon stopped");
    Ok(())
}
