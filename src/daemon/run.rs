use std::sync::Arc;

use serenity::model::id::ChannelId;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tracing::info;

use crate::agents::AgentRunner;
use crate::chat::{ActiveSessions, SessionChat};
use crate::db::{self, GhostDb};
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

    // Phase 1: create directories + user-only files
    crate::config_workspace::bootstrap_workspace_dirs(&config)?;
    info!(workspace = %config.workspace.display(), "config loaded");

    // Check for bundled file changes BEFORE installing
    let changes = crate::bundled::compute_changes(&config.workspace);
    let has_updates = changes.has_updates();

    // Auto-install new files immediately (no conflict possible)
    for file in &changes.new {
        let dest = config.workspace.join(file.path);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&dest, file.content)?;
    }

    crate::tools::shell::spawn_flake_warmup(config.workspace.clone());

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

    // Create bundled update channel if there are updates to review
    let (bundled_tx, bundled_rx) = if has_updates {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        (Some(tx), Some(rx))
    } else {
        (None, None)
    };

    let discord_result = discord::start_discord(
        &config,
        session_chat.clone(),
        db.clone(),
        active_sessions,
        bundled_tx,
    )
    .await?;

    // If there are updates, prompt the user or auto-accept
    if has_updates {
        handle_bundled_updates(&config, &changes, &db, &discord_result, bundled_rx).await?;
    } else {
        // No updates, just save manifest
        crate::bundled::save_manifest(&config.workspace).map_err(|source| {
            crate::config::ConfigError::WriteFile {
                path: config.workspace.clone(),
                source,
            }
        })?;
    }

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

async fn handle_bundled_updates(
    config: &crate::config::Config,
    changes: &crate::bundled::BundledChanges,
    db: &GhostDb,
    discord_result: &Option<(DiscordSender, JoinHandle<()>)>,
    mut bundled_rx: Option<tokio::sync::mpsc::UnboundedReceiver<String>>,
) -> Result<(), GhostError> {
    let decision = if let (Some(rx), Some((sender, _))) = (&mut bundled_rx, discord_result) {
        if let Some(channel_id) = resolve_update_channel(db).await {
            info!(
                changed = changes.changed.len(),
                removed = changes.removed.len(),
                "prompting user for bundled file updates"
            );
            crate::bundled::prompt_updates_via_discord(changes, sender.http(), channel_id, rx).await
        } else {
            info!("no Discord channel found, auto-accepting bundled updates");
            crate::bundled::UpdateDecision::accept_all(changes)
        }
    } else {
        info!("no Discord available, auto-accepting bundled updates");
        crate::bundled::UpdateDecision::accept_all(changes)
    };

    crate::bundled::apply_updates(&config.workspace, changes, &decision).map_err(|source| {
        crate::config::ConfigError::WriteFile {
            path: config.workspace.clone(),
            source,
        }
    })?;

    Ok(())
}

/// Find a Discord channel to post the update dialog in.
async fn resolve_update_channel(db: &GhostDb) -> Option<ChannelId> {
    let interfaces = db::interface_sessions::list_all_interface_sessions(db)
        .await
        .ok()?;
    for iface in interfaces {
        if let Some(id) = iface
            .interface
            .strip_prefix("discord:channel:")
            .and_then(|s| s.parse::<u64>().ok())
        {
            return Some(ChannelId::new(id));
        }
    }
    None
}
