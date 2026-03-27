use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tracing::info;

use crate::config::{SharedConfig, SharedConfigExt};
use crate::db;
use crate::db::GhostDb;

use super::crontab::{CrontabTrigger, load_crontab};
use super::runner::AgentRunner;

/// Debounce delay after agent file changes before reloading.
const FILE_CHANGE_DEBOUNCE: Duration = Duration::from_millis(500);

/// A scheduled Lua agent.
#[derive(Debug)]
struct ScheduleEntry {
    name: String,
    cron: cron::Schedule,
}

/// Tracked state for a scheduled agent.
#[derive(Debug)]
struct TrackedEntry {
    entry: ScheduleEntry,
    next_run: Option<DateTime<Utc>>,
    last_run: Option<DateTime<Utc>>,
}

/// Idle agent entry — Lua agent with trigger=after_idle.
#[derive(Debug)]
struct IdleAgent {
    name: String,
    idle_minutes: u64,
}

/// Spawn the unified agent scheduler. Handles both cron-scheduled agents
/// and idle-triggered agents.
#[tracing::instrument(name = "start scheduler", skip_all)]
pub fn spawn_scheduler(
    agent_runner: Arc<AgentRunner>,
    mut config: SharedConfig,
    db: GhostDb,
    mut shutdown: watch::Receiver<bool>,
    mut idle_trigger_rx: mpsc::Receiver<()>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let cfg = config.current();
        let mut tick_secs = cfg.timing.scheduler_tick_seconds;
        let workspace = cfg.workspace.clone();
        let agents_dir = workspace.join("agents");

        let (mut scheduled, mut idle_agents) = build_entries(&workspace);

        // Set up file watcher on agents/ directory
        let (fs_tx, mut fs_rx) = mpsc::channel::<PathBuf>(64);
        let _watcher = match setup_watcher(&agents_dir, fs_tx) {
            Ok(w) => Some(w),
            Err(e) => {
                tracing::warn!(
                    error = e.to_string(),
                    "failed to start scheduler file watcher",
                );
                None
            }
        };

        info!(
            tick_seconds = tick_secs,
            scheduled_count = scheduled.len(),
            idle_count = idle_agents.len(),
            "unified scheduler started",
        );

        let mut interval = tokio::time::interval(Duration::from_secs(tick_secs));

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    tick_scheduled(&agent_runner, &db, &mut scheduled).await;
                    tick_idle(&agent_runner, &db, &idle_agents).await;
                }
                Some(()) = idle_trigger_rx.recv() => {
                    info!("manual idle trigger received");
                    tick_idle(&agent_runner, &db, &idle_agents).await;
                }
                path = fs_rx.recv() => {
                    if let Some(_path) = path {
                        tokio::time::sleep(FILE_CHANGE_DEBOUNCE).await;
                        while fs_rx.try_recv().is_ok() {}

                        info!("agent files changed, reloading");
                        let (new_scheduled, new_idle) = build_entries(&workspace);
                        scheduled = new_scheduled;
                        idle_agents = new_idle;
                    }
                }
                _ = config.changed() => {
                    let cfg = config.current();
                    let new_tick = cfg.timing.scheduler_tick_seconds;
                    if new_tick != tick_secs {
                        tick_secs = new_tick;
                        interval = tokio::time::interval(Duration::from_secs(tick_secs));
                        info!(tick_seconds = tick_secs, "scheduler tick interval updated");
                    }
                    // Reload agent entries in case workspace agents changed
                    let (new_scheduled, new_idle) = build_entries(&workspace);
                    scheduled = new_scheduled;
                    idle_agents = new_idle;
                }
                _ = shutdown.changed() => {
                    info!("unified scheduler shutting down");
                    break;
                }
            }
        }

        info!("unified scheduler stopped");
    })
}

/// Build all schedule entries from the crontab.
fn build_entries(workspace: &Path) -> (Vec<TrackedEntry>, Vec<IdleAgent>) {
    let now = Utc::now();
    let mut scheduled = Vec::new();
    let mut idle_agents = Vec::new();

    let entries = match load_crontab(workspace) {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!(error = e.clone(), "scheduler: failed to load crontab");
            return (scheduled, idle_agents);
        }
    };

    for entry in entries {
        match entry.kind {
            CrontabTrigger::Cron { ref expr } => {
                let cron_expr = format!("0 {expr}");
                match cron::Schedule::from_str(&cron_expr) {
                    Ok(schedule) => {
                        let next_run = schedule.after(&now).next();
                        scheduled.push(TrackedEntry {
                            entry: ScheduleEntry {
                                name: entry.run.clone(),
                                cron: schedule,
                            },
                            next_run,
                            last_run: None,
                        });
                    }
                    Err(e) => {
                        tracing::warn!(
                            agent = entry.run.clone(),
                            cron = expr.clone(),
                            error = e.to_string(),
                            "scheduler: invalid cron for agent",
                        );
                    }
                }
            }
            CrontabTrigger::Idle { minutes } => {
                idle_agents.push(IdleAgent {
                    name: entry.run.clone(),
                    idle_minutes: minutes,
                });
            }
        }
    }

    (scheduled, idle_agents)
}

/// Check and execute due scheduled entries.
async fn tick_scheduled(agent_runner: &AgentRunner, _db: &GhostDb, entries: &mut [TrackedEntry]) {
    let now = Utc::now();

    for entry in entries.iter_mut() {
        let Some(next_run) = entry.next_run else {
            continue;
        };
        if now < next_run {
            continue;
        }

        let name = &entry.entry.name;

        tracing::info!(agent_name = name.clone(), "executing scheduled agent");

        match agent_runner
            .run(name, "Execute the scheduled agent.", None)
            .await
        {
            Ok(mut result) => {
                agent_runner.spawn_children(&mut result);
                tracing::info!(agent_name = name.clone(), "scheduled agent completed");
            }
            Err(e) => {
                tracing::error!(
                    agent_name = name.clone(),
                    error = e.to_string(),
                    "scheduled agent failed",
                );
            }
        }

        entry.last_run = Some(now);
        entry.next_run = entry.entry.cron.after(&now).next();
    }
}

/// Check idle sessions and trigger after_idle agents.
async fn tick_idle(agent_runner: &AgentRunner, db: &GhostDb, idle_agents: &[IdleAgent]) {
    if idle_agents.is_empty() {
        return;
    }

    let sessions = match db::interface_sessions::list_all_interface_sessions(db).await {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(
                error = e.to_string(),
                "scheduler: failed to list interface sessions for idle check",
            );
            return;
        }
    };

    let now = Utc::now();

    for agent in idle_agents {
        let idle_threshold = chrono::Duration::minutes(agent.idle_minutes as i64);

        for record in &sessions {
            // Only check active sessions
            match db::sessions::get_session(db, &record.session_id).await {
                Ok(s) if s.status == "active" => {}
                _ => continue,
            }

            // Step 1: get last message timestamp
            let last_msg_at = match db::sessions::last_message_at(db, &record.session_id).await {
                Ok(Some(ts)) => ts,
                Ok(None) => continue,
                Err(e) => {
                    tracing::warn!(
                        session_id = record.session_id.clone(),
                        error = e.to_string(),
                        "scheduler: failed to get last message time",
                    );
                    continue;
                }
            };

            let last_msg_dt = match chrono::DateTime::parse_from_rfc3339(&last_msg_at) {
                Ok(dt) => dt.with_timezone(&Utc),
                Err(_) => continue,
            };

            if now - last_msg_dt < idle_threshold {
                continue; // not idle yet
            }

            // Step 2: check if agent already ran for this idle period
            match db::agent_runs::has_run_since(db, &agent.name, &record.session_id, &last_msg_at)
                .await
            {
                Ok(true) => continue, // already handled
                Ok(false) => {}
                Err(e) => {
                    tracing::warn!(
                        agent_name = agent.name.clone(),
                        session_id = record.session_id.clone(),
                        error = e.to_string(),
                        "scheduler: failed to check agent run history",
                    );
                    continue;
                }
            }

            tracing::info!(
                agent_name = agent.name.clone(),
                session_id = record.session_id.clone(),
                idle_minutes = agent.idle_minutes,
                "idle threshold reached, triggering agent",
            );

            match agent_runner
                .run(
                    &agent.name,
                    "Execute after idle period.",
                    Some(&record.session_id),
                )
                .await
            {
                Ok(mut result) => {
                    agent_runner.spawn_children(&mut result);
                    tracing::info!(agent_name = agent.name.clone(), "idle agent completed");
                }
                Err(e) => {
                    tracing::error!(
                        agent_name = agent.name.clone(),
                        error = e.to_string(),
                        "idle agent failed",
                    );
                }
            }
        }
    }
}

fn setup_watcher(
    agents_dir: &Path,
    tx: mpsc::Sender<PathBuf>,
) -> Result<RecommendedWatcher, notify::Error> {
    let mut watcher = notify::recommended_watcher(move |result: Result<Event, _>| {
        if let Ok(event) = result {
            match event.kind {
                EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) => {
                    for path in event.paths {
                        let _ = tx.try_send(path);
                    }
                }
                _ => {}
            }
        }
    })?;

    if agents_dir.exists() {
        watcher.watch(agents_dir, RecursiveMode::Recursive)?;
    }

    Ok(watcher)
}
