use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tracing::info;

use crate::config::Config;
use crate::db;
use crate::db::GhostDb;
use crate::scripting::AgentContext;
use crate::scripting::types::AgentTrigger;

use super::loader::{discover_agents, load_agent, load_agent_with_host};
use super::runner::TaskRunner;

/// A scheduled Lua agent.
#[derive(Debug)]
struct ScheduleEntry {
    name: String,
    cron: cron::Schedule,
    #[allow(dead_code)]
    has_should_trigger: bool,
}

/// Tracked state for a scheduled agent.
#[derive(Debug)]
struct TrackedEntry {
    entry: ScheduleEntry,
    next_run: Option<DateTime<Utc>>,
    #[allow(dead_code)]
    last_run: Option<DateTime<Utc>>,
}

/// Idle agent entry — Lua agent with trigger=after_idle.
#[derive(Debug)]
#[allow(dead_code)]
struct IdleAgent {
    name: String,
    idle_minutes: u64,
    has_should_trigger: bool,
}

/// Spawn the unified agent scheduler. Handles both cron-scheduled agents
/// and idle-triggered agents.
#[tracing::instrument(name = "start scheduler", skip_all)]
pub fn spawn_scheduler(
    task_runner: Arc<TaskRunner>,
    config: Config,
    db: GhostDb,
    mut shutdown: watch::Receiver<bool>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let tick_secs = config.timing.scheduler_tick_seconds;
        let workspace = config.workspace.clone();
        let agents_dir = workspace.join("agents");

        let (mut scheduled, mut idle_agents) = build_entries(&workspace);

        // Set up file watcher on agents/ directory
        let (fs_tx, mut fs_rx) = mpsc::channel::<PathBuf>(64);
        let _watcher = match setup_watcher(&agents_dir, fs_tx) {
            Ok(w) => Some(w),
            Err(e) => {
                logfire::warn!(
                    "failed to start scheduler file watcher",
                    error = e.to_string(),
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
                    tick_scheduled(&task_runner, &db, &workspace, &mut scheduled).await;
                    tick_idle(&task_runner, &db, &workspace, &idle_agents).await;
                }
                path = fs_rx.recv() => {
                    if let Some(_path) = path {
                        tokio::time::sleep(Duration::from_millis(500)).await;
                        while fs_rx.try_recv().is_ok() {}

                        info!("agent files changed, reloading");
                        let (new_scheduled, new_idle) = build_entries(&workspace);
                        scheduled = new_scheduled;
                        idle_agents = new_idle;
                    }
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

/// Build all schedule entries from the workspace.
fn build_entries(workspace: &Path) -> (Vec<TrackedEntry>, Vec<IdleAgent>) {
    let now = Utc::now();
    let mut scheduled = Vec::new();
    let mut idle_agents = Vec::new();

    let agents = discover_agents(workspace);
    for agent_info in &agents {
        let config = match load_agent(workspace, &agent_info.name) {
            Ok(c) => c,
            Err(e) => {
                logfire::warn!(
                    "scheduler: failed to load agent",
                    agent = agent_info.name.clone(),
                    error = e.to_string(),
                );
                continue;
            }
        };

        match &config.trigger {
            AgentTrigger::Schedule { cron } => {
                let cron_expr = format!("0 {cron}");
                match cron::Schedule::from_str(&cron_expr) {
                    Ok(schedule) => {
                        let next_run = schedule.after(&now).next();
                        scheduled.push(TrackedEntry {
                            entry: ScheduleEntry {
                                name: config.name.clone(),
                                cron: schedule,
                                has_should_trigger: config.has_should_trigger,
                            },
                            next_run,
                            last_run: None,
                        });
                    }
                    Err(e) => {
                        logfire::warn!(
                            "scheduler: invalid cron for agent",
                            agent = config.name.clone(),
                            cron = cron.clone(),
                            error = e.to_string(),
                        );
                    }
                }
            }
            AgentTrigger::AfterIdle { minutes } => {
                idle_agents.push(IdleAgent {
                    name: config.name.clone(),
                    idle_minutes: *minutes,
                    has_should_trigger: config.has_should_trigger,
                });
            }
            _ => {} // Dispatch and AfterAgent handled elsewhere
        }
    }

    (scheduled, idle_agents)
}

/// Check and execute due scheduled entries.
async fn tick_scheduled(
    task_runner: &TaskRunner,
    db: &GhostDb,
    workspace: &Path,
    entries: &mut [TrackedEntry],
) {
    let now = Utc::now();

    for entry in entries.iter_mut() {
        let Some(next_run) = entry.next_run else {
            continue;
        };
        if now < next_run {
            continue;
        }

        let name = &entry.entry.name;

        // Check should_trigger hook
        if entry.entry.has_should_trigger {
            match load_agent_with_host(workspace, name) {
                Ok((_config, host)) => {
                    let ctx = AgentContext {
                        db: db.clone(),
                        workspace: workspace.to_path_buf(),
                        agent_slug: name.clone(),
                        session_id: String::new(),
                        trigger_session_id: None,
                        trigger_agent_name: None,
                    };
                    match host.call_should_trigger(ctx) {
                        Ok(false) => {
                            logfire::debug!(
                                "scheduled agent skipped by should_trigger",
                                agent_name = name.clone(),
                            );
                            entry.last_run = Some(now);
                            entry.next_run = entry.entry.cron.after(&now).next();
                            continue;
                        }
                        Err(e) => {
                            logfire::warn!(
                                "should_trigger hook error, proceeding anyway",
                                agent_name = name.clone(),
                                error = e.to_string(),
                            );
                        }
                        Ok(true) => {}
                    }
                }
                Err(e) => {
                    logfire::warn!(
                        "failed to load agent for should_trigger check",
                        agent_name = name.clone(),
                        error = e.to_string(),
                    );
                }
            }
        }

        logfire::info!("executing scheduled agent", agent_name = name.clone());

        match task_runner
            .run_to_completion(name, "Execute the scheduled agent.", None)
            .await
        {
            Ok((_findings, _meta)) => {
                logfire::info!("scheduled agent completed", agent_name = name.clone());
            }
            Err(e) => {
                logfire::error!(
                    "scheduled agent failed",
                    agent_name = name.clone(),
                    error = e.to_string(),
                );
            }
        }

        entry.last_run = Some(now);
        entry.next_run = entry.entry.cron.after(&now).next();
    }
}

/// Check idle sessions and trigger after_idle agents.
async fn tick_idle(
    task_runner: &TaskRunner,
    db: &GhostDb,
    workspace: &Path,
    idle_agents: &[IdleAgent],
) {
    if idle_agents.is_empty() {
        return;
    }

    let sessions = match db::interface_sessions::list_all_interface_sessions(db).await {
        Ok(s) => s,
        Err(e) => {
            logfire::warn!(
                "scheduler: failed to list interface sessions for idle check",
                error = e.to_string(),
            );
            return;
        }
    };

    let now = Utc::now();

    for agent in idle_agents {
        let idle_threshold = chrono::Duration::minutes(agent.idle_minutes as i64);

        // Check should_trigger hook once per agent (not per session)
        if agent.has_should_trigger {
            match load_agent_with_host(workspace, &agent.name) {
                Ok((_config, host)) => {
                    let ctx = AgentContext {
                        db: db.clone(),
                        workspace: workspace.to_path_buf(),
                        agent_slug: agent.name.clone(),
                        session_id: String::new(),
                        trigger_session_id: None,
                        trigger_agent_name: None,
                    };
                    match host.call_should_trigger(ctx) {
                        Ok(false) => {
                            logfire::debug!(
                                "idle agent skipped by should_trigger",
                                agent_name = agent.name.clone(),
                            );
                            continue;
                        }
                        Err(e) => {
                            logfire::warn!(
                                "should_trigger hook error, proceeding anyway",
                                agent_name = agent.name.clone(),
                                error = e.to_string(),
                            );
                        }
                        Ok(true) => {}
                    }
                }
                Err(e) => {
                    logfire::warn!(
                        "failed to load agent for should_trigger check",
                        agent_name = agent.name.clone(),
                        error = e.to_string(),
                    );
                }
            }
        }

        for record in &sessions {
            let session = match db::sessions::get_session(db, &record.session_id).await {
                Ok(s) => s,
                Err(_) => continue,
            };

            if session.status != "active" {
                continue;
            }

            let last_activity = chrono::DateTime::parse_from_rfc3339(&session.last_activity_at)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());

            if now - last_activity < idle_threshold {
                continue;
            }

            logfire::info!(
                "idle threshold reached, triggering agent",
                agent_name = agent.name.clone(),
                session_id = record.session_id.clone(),
                idle_minutes = agent.idle_minutes,
            );

            match task_runner
                .run_to_completion(
                    &agent.name,
                    "Execute after idle period.",
                    Some(&record.session_id),
                )
                .await
            {
                Ok((_findings, _meta)) => {
                    logfire::info!("idle agent completed", agent_name = agent.name.clone(),);
                }
                Err(e) => {
                    logfire::error!(
                        "idle agent failed",
                        agent_name = agent.name.clone(),
                        error = e.to_string(),
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
