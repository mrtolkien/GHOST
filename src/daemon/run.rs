use std::sync::Arc;

use tokio::sync::watch;
use tokio::task::JoinHandle;
use tracing::info;

use crate::agents::TaskRunner;
use crate::chat::SessionChat;
use crate::embeddings::EmbeddingClient;
use crate::error::GhostError;
use crate::interfaces::discord::{self, DiscordSender};

pub async fn run() -> Result<(), GhostError> {
    let (shutdown_tx, watcher_handle, scheduler_handle, discord_result, agent_watcher_handle) =
        boot().await?;

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

    // Signal shutdown to watcher, scheduler, and agent watcher
    let _ = shutdown_tx.send(true);
    let _ = watcher_handle.await;
    let _ = scheduler_handle.await;
    if let Some(aw_handle) = agent_watcher_handle {
        let _ = aw_handle.await;
    }

    info!("GHOST daemon stopped");
    Ok(())
}

type BootResult = (
    watch::Sender<bool>,
    JoinHandle<()>,
    JoinHandle<()>,
    Option<(DiscordSender, JoinHandle<()>)>,
    Option<JoinHandle<()>>,
);

#[tracing::instrument(name = "boot ghost", skip_all)]
pub async fn boot() -> Result<BootResult, GhostError> {
    info!("loading config");
    let config = crate::config::load()?;
    crate::config_workspace::bootstrap_workspace(&config)?;
    info!(workspace = %config.workspace.display(), "config loaded");

    info!("connecting to database");
    let db = crate::db::connect(&config.workspace, config.embeddings.dimension).await?;
    info!("database ready");

    // Log knowledge counts for boot diagnostics
    let notes = crate::db::knowledge::count_notes(&db).await.unwrap_or(0);
    let references = crate::db::knowledge::count_references(&db)
        .await
        .unwrap_or(0);
    let diary = crate::db::knowledge::count_diary(&db).await.unwrap_or(0);
    let embeddings = crate::db::embeddings::count_embeddings(&db)
        .await
        .unwrap_or(0);
    info!(
        notes,
        references, diary, embeddings, "knowledge stats at boot"
    );

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
    let watcher_handle = super::watcher::spawn_watcher(
        db.clone(),
        config.workspace.clone(),
        config.embeddings.clone(),
        shutdown_rx.clone(),
    );

    // Create agent runner (shared between SessionChat, scheduler, agent watcher)
    let task_runner = Arc::new(TaskRunner::new(db.clone(), config.clone()));

    // Spawn the unified scheduler (handles both cron jobs and idle agents)
    let scheduler_handle = crate::agents::scheduler::spawn_scheduler(
        Arc::clone(&task_runner),
        config.clone(),
        db.clone(),
        shutdown_rx.clone(),
    );

    let session_chat = Arc::new(
        SessionChat::from_config(db.clone(), config.clone())?
            .with_task_runner(Arc::clone(&task_runner)),
    );

    let discord_result = discord::start_discord(&config, session_chat.clone(), db.clone()).await?;

    // Spawn agent watcher (only if Discord is available)
    let agent_watcher_handle = if let Some((ref sender, _)) = discord_result {
        let discord_sender = Arc::new(sender.clone());

        Some(crate::agents::watcher::spawn_task_watcher(
            Arc::clone(&task_runner),
            Arc::clone(&session_chat),
            Arc::clone(&discord_sender),
            db.clone(),
            config.workspace.clone(),
            shutdown_rx.clone(),
        ))
    } else {
        None
    };

    Ok((
        shutdown_tx,
        watcher_handle,
        scheduler_handle,
        discord_result,
        agent_watcher_handle,
    ))
}
