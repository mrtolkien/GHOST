use std::sync::Arc;

use tokio::sync::watch;
use tokio::task::JoinHandle;
use tracing::info;

use crate::agents::AgentRunner;
use crate::chat::{ActiveSessions, SessionChat};
use crate::embeddings::EmbeddingClient;
use crate::error::GhostError;
use crate::interfaces::discord::{self, DiscordSender};

pub async fn run() -> Result<(), GhostError> {
    let (
        shutdown_tx,
        watcher_handle,
        reconcile_handle,
        scheduler_handle,
        discord_result,
        event_handler_handle,
    ) = boot().await?;

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

    let _ = shutdown_tx.send(true);
    let _ = watcher_handle.await;
    let _ = reconcile_handle.await;
    let _ = scheduler_handle.await;
    let _ = event_handler_handle.await;

    info!("GHOST daemon stopped");
    Ok(())
}

type BootResult = (
    watch::Sender<bool>,
    JoinHandle<()>,
    JoinHandle<()>,
    JoinHandle<()>,
    Option<(DiscordSender, JoinHandle<()>)>,
    JoinHandle<()>,
);

#[tracing::instrument(name = "boot ghost", skip_all)]
pub async fn boot() -> Result<BootResult, GhostError> {
    info!("loading config");
    let config = crate::config::load()?;
    crate::config_workspace::bootstrap_workspace(&config)?;
    info!(workspace = %config.workspace.display(), "config loaded");

    if let Err(e) = crate::tools::shell::run_home_manager_switch(&config.workspace).await {
        logfire::warn!("home-manager switch failed at boot", error = e.to_string());
    }

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

    // Spawn hourly embedding reconciliation
    let reconcile_handle = super::watcher::spawn_reconciliation_loop(
        db.clone(),
        config.embeddings.clone(),
        shutdown_rx.clone(),
    );

    // Create session event channel (background tasks → event handler)
    let (event_tx, event_rx) = crate::events::channel();

    // Create agent runner with event sender
    let agent_runner = Arc::new(AgentRunner::new(
        db.clone(),
        config.clone(),
        Some(event_tx.clone()),
    ));

    // Spawn the unified scheduler (handles both cron jobs and idle agents)
    let scheduler_handle = crate::agents::scheduler::spawn_scheduler(
        Arc::clone(&agent_runner),
        config.clone(),
        db.clone(),
        shutdown_rx.clone(),
    );

    let active_sessions: ActiveSessions = std::sync::Arc::new(dashmap::DashMap::new());

    let session_chat = Arc::new(
        SessionChat::from_config(db.clone(), config.clone())?
            .with_agent_runner(Arc::clone(&agent_runner))
            .with_event_sender(event_tx)
            .with_active_sessions(active_sessions.clone()),
    );

    let discord_result =
        discord::start_discord(&config, session_chat.clone(), db.clone(), active_sessions).await?;

    let discord_sender_arc = discord_result
        .as_ref()
        .map(|(sender, _)| Arc::new(sender.clone()));

    // Spawn unified event handler (replaces agent_watcher + completion_watcher)
    let event_handler_handle = super::event_handler::spawn_event_handler(
        event_rx,
        Arc::clone(&session_chat),
        discord_sender_arc,
        db.clone(),
        shutdown_rx.clone(),
    );

    Ok((
        shutdown_tx,
        watcher_handle,
        reconcile_handle,
        scheduler_handle,
        discord_result,
        event_handler_handle,
    ))
}
