use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serenity::model::id::ChannelId;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tracing::info;

#[derive(Debug, thiserror::Error)]
#[error("system did not settle within {0:?}")]
pub struct SettleTimeout(std::time::Duration);

use crate::agents::AgentRunner;
use crate::chat::{ActiveSessions, SessionChat};
use crate::config::{Config, SharedConfig};
use crate::db::{self, GhostDb};
use crate::embeddings::EmbeddingClient;
use crate::error::GhostError;
use crate::interfaces::discord::{self, DiscordHandle};

/// Handle to a running GHOST daemon. Returned by `boot()`.
#[allow(dead_code)]
pub struct DaemonHandle {
    pub session_chat: Arc<SessionChat>,
    pub db: GhostDb,
    pub config: Config,
    pub agent_runner: Arc<AgentRunner>,
    pub active_sessions: ActiveSessions,
    shutdown_tx: watch::Sender<bool>,
    handles: Vec<JoinHandle<()>>,
    discord: Option<DiscordHandle>,
    idle_trigger_tx: tokio::sync::mpsc::Sender<()>,
    watcher_busy: Arc<AtomicBool>,
}

impl DaemonHandle {
    /// Returns true when all subsystems are idle.
    pub fn is_idle(&self) -> bool {
        self.active_sessions.is_empty()
            && self.agent_runner.active_count() == 0
            && !self.watcher_busy.load(Ordering::Relaxed)
            && crate::tools::shell::background_shell_count() == 0
    }

    /// Trigger idle agents immediately (reflection, etc.).
    pub async fn trigger_idle_agents(&self) {
        let _ = self.idle_trigger_tx.send(()).await;
    }

    /// Wait until all subsystems are idle, or timeout (default 180s).
    pub async fn settle(&self) -> Result<(), SettleTimeout> {
        self.settle_with_timeout(std::time::Duration::from_secs(180))
            .await
    }

    /// Wait until all subsystems are idle, or timeout after `timeout`.
    pub async fn settle_with_timeout(
        &self,
        timeout: std::time::Duration,
    ) -> Result<(), SettleTimeout> {
        let deadline = tokio::time::Instant::now() + timeout;
        let poll = std::time::Duration::from_millis(500);

        loop {
            if self.is_idle() {
                // Stay idle for one more poll to catch races
                tokio::time::sleep(poll).await;
                if self.is_idle() {
                    return Ok(());
                }
            }

            if tokio::time::Instant::now() >= deadline {
                return Err(SettleTimeout(timeout));
            }

            tokio::time::sleep(poll).await;
        }
    }

    /// Signal all subsystems to shut down and wait for them.
    pub async fn shutdown(self) {
        let _ = self.shutdown_tx.send(true);
        for h in self.handles {
            let _ = h.await;
        }
        if let Some(discord) = self.discord {
            discord.shutdown().await;
        }
    }
}

pub async fn run() -> Result<(), GhostError> {
    let config = crate::config::load()?;

    // Race boot against shutdown signals so SIGTERM during boot works
    let handle = tokio::select! {
        result = boot_with_config(config) => result?,
        _ = shutdown_signal() => {
            info!("signal received during boot, exiting...");
            return Ok(());
        }
    };

    if handle.discord.is_some() {
        info!("GHOST daemon running — press Ctrl+C to stop");
    } else {
        info!("No interfaces enabled. Waiting for Ctrl+C...");
    }

    shutdown_signal().await;
    info!("shutting down...");

    handle.shutdown().await;

    info!("GHOST daemon stopped");
    Ok(())
}

/// Wait for either SIGINT (Ctrl+C) or SIGTERM (`ghost reboot`).
async fn shutdown_signal() {
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("failed to register SIGTERM handler");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = sigterm.recv() => {}
    }
}

#[tracing::instrument(name = "boot ghost", skip_all)]
pub async fn boot() -> Result<DaemonHandle, GhostError> {
    info!("loading config");
    let config = crate::config::load()?;
    boot_with_config(config).await
}

/// Boot the daemon with a pre-built config (for tests and programmatic use).
#[tracing::instrument(name = "boot ghost", skip_all)]
pub async fn boot_with_config(config: Config) -> Result<DaemonHandle, GhostError> {
    // Phase 1: create directories + user-only files
    crate::config_workspace::bootstrap_workspace_dirs(&config)?;

    info!(workspace = %config.workspace.display(), "config loaded");

    // Install bundled docs to references/ghost/docs/ (silently, content-hash checked)
    if config.install_bundled_docs {
        match crate::bundled::install_docs(&config.workspace) {
            Ok(0) => {}
            Ok(n) => info!(n, "updated bundled docs"),
            Err(e) => logfire::warn!("failed to install bundled docs", error = e.to_string()),
        }
    }

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

    if let Err(e) = crate::tools::shell::rebuild_shell_env(&config.workspace).await {
        logfire::warn!("nix shell setup failed at boot", error = e.to_string());
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

    // Boot reconciliation: discover missed files, then embed
    let client = EmbeddingClient::new(&config.embeddings);
    if client.is_available().await {
        info!("running boot reconciliation");
        match crate::embeddings::pipeline::reconcile_filesystem(&db, &config.workspace).await {
            Ok((discovered, embed_requests)) => {
                if discovered > 0 {
                    info!(discovered, "boot: discovered untracked files");
                }
                if !embed_requests.is_empty() {
                    match crate::embeddings::pipeline::embed_sources(&client, &db, embed_requests)
                        .await
                    {
                        Ok(embedded) => {
                            info!(embedded, "boot reconciliation complete");
                        }
                        Err(e) => {
                            logfire::warn!("boot embedding failed", error = e.to_string(),);
                        }
                    }
                }
            }
            Err(e) => {
                logfire::warn!("boot reconciliation failed", error = e.to_string());
            }
        }
    } else {
        logfire::warn!("Ollama unavailable — skipping boot reconciliation");
    }

    // Spawn file watcher for automatic embedding on content changes
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let watcher_busy = Arc::new(AtomicBool::new(false));
    let watcher_handle = super::watcher::spawn_watcher(
        db.clone(),
        config.workspace.clone(),
        config.embeddings.clone(),
        shutdown_rx.clone(),
        Arc::clone(&watcher_busy),
    );

    // Spawn hourly embedding reconciliation
    let reconcile_handle = super::watcher::spawn_reconciliation_loop(
        db.clone(),
        config.workspace.clone(),
        config.embeddings.clone(),
        shutdown_rx.clone(),
    );

    // Create session event channel (background tasks → event handler)
    let (event_tx, event_rx) = crate::events::channel();

    // Create shared config for hot-reload (sender held for future reload support)
    let (_config_tx, shared_config): (crate::config::ConfigSender, SharedConfig) =
        tokio::sync::watch::channel(Arc::new(config.clone()));

    // Create agent runner with event sender
    let agent_runner = Arc::new(AgentRunner::new(
        db.clone(),
        shared_config.clone(),
        Some(event_tx.clone()),
    ));

    // Spawn the unified scheduler (handles both cron jobs and idle agents)
    let (idle_trigger_tx, idle_trigger_rx) = tokio::sync::mpsc::channel::<()>(8);
    let scheduler_handle = crate::agents::scheduler::spawn_scheduler(
        Arc::clone(&agent_runner),
        shared_config.clone(),
        db.clone(),
        shutdown_rx.clone(),
        idle_trigger_rx,
    );

    let active_sessions: ActiveSessions = std::sync::Arc::new(dashmap::DashMap::new());

    // Create confirmation channel for file edit validation
    let (confirmation_tx, confirmation_rx) = crate::tools::confirmation::channel();

    let session_chat = Arc::new(
        SessionChat::from_config(db.clone(), shared_config.clone())?
            .with_agent_runner(Arc::clone(&agent_runner))
            .with_event_sender(event_tx)
            .with_active_sessions(active_sessions.clone())
            .with_confirmation_tx(confirmation_tx),
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
        active_sessions.clone(),
        bundled_tx,
        Some(confirmation_rx),
    )
    .await?;

    // If there are updates, prompt the user or auto-accept
    if has_updates {
        handle_bundled_updates(&config, &changes, &db, &discord_result, bundled_rx).await?;
    }

    let discord_sender_arc = discord_result.as_ref().map(|d| Arc::new(d.sender.clone()));

    // Spawn unified event handler (replaces agent_watcher + completion_watcher)
    let event_handler_handle = super::event_handler::spawn_event_handler(
        event_rx,
        Arc::clone(&session_chat),
        discord_sender_arc,
        db.clone(),
        shutdown_rx.clone(),
    );

    Ok(DaemonHandle {
        session_chat,
        db,
        config,
        agent_runner,
        active_sessions,
        shutdown_tx,
        handles: vec![
            watcher_handle,
            reconcile_handle,
            scheduler_handle,
            event_handler_handle,
        ],
        discord: discord_result,
        idle_trigger_tx,
        watcher_busy,
    })
}

async fn handle_bundled_updates(
    config: &crate::config::Config,
    changes: &crate::bundled::BundledChanges,
    db: &GhostDb,
    discord_result: &Option<DiscordHandle>,
    mut bundled_rx: Option<tokio::sync::mpsc::UnboundedReceiver<String>>,
) -> Result<(), GhostError> {
    let decision = if let (Some(rx), Some(discord)) = (&mut bundled_rx, discord_result) {
        if let Some(channel_id) = resolve_update_channel(db).await {
            info!(
                merges = changes.clean_merges.len(),
                conflicts = changes.conflicts.len(),
                removed = changes.removed.len(),
                "prompting user for bundled file updates"
            );
            crate::bundled::prompt_updates_via_discord(
                changes,
                discord.sender.http(),
                channel_id,
                rx,
            )
            .await
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
