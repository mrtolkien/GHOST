use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serenity::model::id::ChannelId;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tracing::{Instrument, info};

const DEFAULT_SETTLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(180);
const SETTLE_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(500);

#[derive(Debug, thiserror::Error)]
#[error("system did not settle within {0:?}")]
pub struct SettleTimeout(std::time::Duration);

use crate::agents::AgentRunner;
use crate::chat::{ActiveSessions, SessionChat};
use crate::config::{Config, ConfigSender, SharedConfig, SharedConfigExt};
use crate::db::{self, GhostDb};
use crate::embeddings::EmbeddingClient;
use crate::error::GhostError;
use crate::interfaces::discord::{self, DiscordHandle};

/// Handle to a running GHOST daemon. Returned by `boot()`.
#[allow(
    dead_code,
    reason = "private fields are held for RAII lifetime and accessed only within the impl (shutdown, settle, config reload)"
)]
pub struct DaemonHandle {
    pub session_chat: Arc<SessionChat>,
    pub db: GhostDb,
    pub config: SharedConfig,
    config_tx: ConfigSender,
    pub agent_runner: Arc<AgentRunner>,
    pub active_sessions: ActiveSessions,
    shutdown_tx: watch::Sender<bool>,
    handles: Vec<JoinHandle<()>>,
    discord: Option<DiscordHandle>,
    idle_trigger_tx: tokio::sync::mpsc::Sender<()>,
    watcher_busy: Arc<AtomicBool>,
    reconciliation_in_progress: Arc<AtomicBool>,
}

impl DaemonHandle {
    /// Returns true when all subsystems are idle.
    pub fn is_idle(&self) -> bool {
        self.active_sessions.is_empty()
            && self.agent_runner.active_count() == 0
            && !self.watcher_busy.load(Ordering::Relaxed)
            && !self.reconciliation_in_progress.load(Ordering::Relaxed)
            && crate::tools::shell::background_shell_count() == 0
    }

    /// Trigger idle agents immediately (reflection, etc.).
    pub async fn trigger_idle_agents(&self) {
        let _ = self.idle_trigger_tx.send(()).await;
    }

    /// Wait until all subsystems are idle, or timeout.
    pub async fn settle(&self) -> Result<(), SettleTimeout> {
        self.settle_with_timeout(DEFAULT_SETTLE_TIMEOUT).await
    }

    /// Wait until all subsystems are idle, or timeout after `timeout`.
    pub async fn settle_with_timeout(
        &self,
        timeout: std::time::Duration,
    ) -> Result<(), SettleTimeout> {
        let deadline = tokio::time::Instant::now() + timeout;
        let poll = SETTLE_POLL_INTERVAL;

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

    // Prevent multiple daemon instances from running in the same workspace
    super::pid_file::acquire(&config.workspace)?;

    // Race boot against shutdown signals so SIGTERM during boot works
    let handle = tokio::select! {
        result = boot_with_config(config.clone()) => result?,
        _ = shutdown_signal() => {
            info!("signal received during boot, exiting...");
            super::pid_file::release(&config.workspace);
            return Ok(());
        }
    };

    if handle.discord.is_some() {
        info!("GHOST daemon running — press Ctrl+C to stop");
    } else {
        info!("No interfaces enabled. Waiting for Ctrl+C...");
    }

    let mut sighup = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())
        .expect("failed to register SIGHUP handler");

    loop {
        tokio::select! {
                    _ = shutdown_signal() => break,
                    _ = sighup.recv() => {
                        info!("SIGHUP received, reloading config...");
                        match crate::config::reload() {
                            Ok(new_config) => {
                                let current = handle.config.current();
                                match crate::config::validate_reload(&current, &new_config) {
                                    Ok(()) => {
                                        handle.config_tx.send(Arc::new(new_config)).ok();
                                        info!("config reloaded successfully");
                                    }
                                    Err(e) => {
                                        tracing::warn!(
                                            error = e.to_string(),
                                            "config reload rejected",
        );
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::warn!(
                                    error = e.to_string(),
                                    "config reload failed",
        );
                            }
                        }
                    }
                }
    }
    info!("shutting down...");

    let workspace = handle.config.current().workspace.clone();
    handle.shutdown().await;
    super::pid_file::release(&workspace);

    info!("GHOST daemon stopped");
    Ok(())
}

/// Wait for either SIGINT (Ctrl+C) or SIGTERM.
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
    Box::pin(boot_with_config(config)).await
}

/// Boot the daemon with a pre-built config (for tests and programmatic use).
#[tracing::instrument(name = "boot ghost", skip_all)]
pub async fn boot_with_config(config: Config) -> Result<DaemonHandle, GhostError> {
    // Phase 1: workspace bootstrap (dirs, bundled files)
    let (changes, has_interactive) = {
        let _span = tracing::info_span!("bootstrap workspace").entered();

        crate::config_workspace::bootstrap_workspace_dirs(&config)?;
        info!(workspace = %config.workspace.display(), "config loaded");

        if config.install_bundled_docs {
            match crate::bundled::install_docs(&config.workspace) {
                Ok(0) => {}
                Ok(n) => info!(n, "updated bundled docs"),
                Err(e) => tracing::warn!(error = e.to_string(), "failed to install bundled docs"),
            }
        }

        let changes = crate::bundled::compute_changes(&config.workspace);
        let has_interactive = changes.has_interactive_updates();
        crate::bundled::apply_silent_updates(&config.workspace, &changes)?;

        (changes, has_interactive)
    };

    // Phase 1b: rebuild nix shell
    async {
        if let Err(e) = crate::tools::shell::rebuild_shell_env(&config.workspace).await {
            tracing::warn!(error = e.clone(), "nix shell setup failed at boot");
        }
    }
    .instrument(tracing::info_span!("rebuild nix shell"))
    .await;

    // Phase 2: database connect
    let db = async {
        info!("connecting to database");
        let db = crate::db::connect(&config.workspace, config.embeddings.dimension).await?;
        info!("database ready");
        Ok::<_, GhostError>(db)
    }
    .instrument(tracing::info_span!("connect database"))
    .await?;

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

    // Boot reconciliation: spawn in background so boot is not blocked
    let reconciliation_in_progress = Arc::new(AtomicBool::new(false));
    {
        let client = EmbeddingClient::new(&config.embeddings);
        let db_bg = db.clone();
        let workspace_bg = config.workspace.clone();
        let flag = Arc::clone(&reconciliation_in_progress);

        if client.is_available().await {
            flag.store(true, Ordering::Release);
            tokio::spawn(async move {
                info!("running boot reconciliation (background)");
                match Box::pin(crate::embeddings::pipeline::reconcile_filesystem(
                    &db_bg,
                    &workspace_bg,
                ))
                .await
                {
                    Ok((discovered, embed_requests)) => {
                        if discovered > 0 {
                            info!(discovered, "boot: discovered untracked files");
                        }
                        if !embed_requests.is_empty() {
                            match crate::embeddings::pipeline::embed_sources(
                                &client,
                                &db_bg,
                                embed_requests,
                            )
                            .await
                            {
                                Ok(embedded) => {
                                    info!(embedded, "boot reconciliation complete");
                                }
                                Err(e) => {
                                    tracing::warn!(error = e.to_string(), "boot embedding failed",);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!(error = e.to_string(), "boot reconciliation failed");
                    }
                }
                flag.store(false, Ordering::Release);
            });
        } else {
            tracing::warn!("Ollama unavailable — skipping boot reconciliation");
        }
    }

    // Create shared config for hot-reload
    let (config_tx, shared_config): (ConfigSender, SharedConfig) =
        tokio::sync::watch::channel(Arc::new(config.clone()));

    // Spawn file watcher for automatic embedding on content changes
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let watcher_busy = Arc::new(AtomicBool::new(false));
    let watcher_handle = super::watcher::spawn_watcher(
        db.clone(),
        shared_config.clone(),
        shutdown_rx.clone(),
        Arc::clone(&watcher_busy),
        Arc::clone(&reconciliation_in_progress),
    );

    // Spawn hourly embedding reconciliation
    let reconcile_handle = super::watcher::spawn_reconciliation_loop(
        db.clone(),
        shared_config.clone(),
        shutdown_rx.clone(),
        Arc::clone(&reconciliation_in_progress),
    );

    // Create session event channel (background tasks → event handler)
    let (event_tx, event_rx) = crate::events::channel();

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

    // Create bundled update channel if there are interactive updates to review
    let (bundled_tx, bundled_rx) = if has_interactive {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        (Some(tx), Some(rx))
    } else {
        (None, None)
    };

    let discord_result = discord::start_discord(
        &shared_config,
        session_chat.clone(),
        db.clone(),
        active_sessions.clone(),
        bundled_tx,
        Some(confirmation_rx),
    )
    .await?;

    // If there are interactive updates (conflicts, modified removals), prompt user
    if has_interactive {
        let cfg = shared_config.current();
        Box::pin(handle_bundled_interactive_updates(
            &cfg,
            &changes,
            &db,
            &discord_result,
            bundled_rx,
        ))
        .await?;
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
        config: shared_config,
        config_tx,
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
        reconciliation_in_progress,
    })
}

async fn handle_bundled_interactive_updates(
    config: &crate::config::Config,
    changes: &crate::bundled::BundledChanges,
    db: &GhostDb,
    discord_result: &Option<DiscordHandle>,
    mut bundled_rx: Option<tokio::sync::mpsc::UnboundedReceiver<String>>,
) -> Result<(), GhostError> {
    let decision = if let (Some(rx), Some(discord)) = (&mut bundled_rx, discord_result) {
        if let Some(channel_id) = resolve_update_channel(db).await {
            info!(
                conflicts = changes.conflicts.len(),
                modified_removals = changes.modified_removals.len(),
                "prompting user for bundled file conflicts"
            );
            Box::pin(crate::bundled::prompt_interactive_updates_via_discord(
                changes,
                discord.sender.http(),
                channel_id,
                rx,
            ))
            .await
        } else {
            info!("no Discord channel found, keeping user versions for conflicts");
            crate::bundled::InteractiveDecision::reject_all()
        }
    } else {
        info!("no Discord available, keeping user versions for conflicts");
        crate::bundled::InteractiveDecision::reject_all()
    };

    crate::bundled::apply_interactive_updates(&config.workspace, changes, &decision).map_err(
        |source| crate::config::ConfigError::WriteFile {
            path: config.workspace.clone(),
            source,
        },
    )?;

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
