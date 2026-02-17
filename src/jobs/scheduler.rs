use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{DateTime, Utc};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;
use tracing::info;

use crate::chat::SessionChat;
use crate::config::Config;
use crate::db::GhostDb;
use crate::providers::provider_for_alias;
use crate::tools::{ToolManager, ToolSet};

use super::definition::{JobDefinition, JobToolSet, next_run_after, parse_job_file};
use super::error::JobError;

#[derive(Debug)]
pub struct LoadedJob {
    pub definition: JobDefinition,
    pub next_run: Option<DateTime<Utc>>,
    pub last_run: Option<DateTime<Utc>>,
}

#[tracing::instrument(skip_all, fields(workspace = %workspace.display()))]
pub fn load_all_jobs(workspace: &Path) -> Vec<LoadedJob> {
    let jobs_dir = workspace.join("jobs");
    let entries = match std::fs::read_dir(&jobs_dir) {
        Ok(entries) => entries,
        Err(e) => {
            logfire::warn!(
                "cannot read jobs directory",
                path = jobs_dir.display().to_string(),
                error = e.to_string(),
            );
            return Vec::new();
        }
    };

    let now = Utc::now();
    let mut jobs = Vec::new();

    for entry in entries {
        let Ok(entry) = entry else { continue };
        let path = entry.path();

        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }

        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                logfire::warn!(
                    "cannot read job file",
                    path = path.display().to_string(),
                    error = e.to_string(),
                );
                continue;
            }
        };

        match parse_job_file(&path, &content) {
            Ok(def) => {
                let next_run = if def.enabled {
                    next_run_after(&def.schedule, &now)
                } else {
                    None
                };
                jobs.push(LoadedJob {
                    definition: def,
                    next_run,
                    last_run: None,
                });
            }
            Err(e) => {
                logfire::warn!(
                    "invalid job file",
                    path = path.display().to_string(),
                    error = e.to_string(),
                );
            }
        }
    }

    info!(count = jobs.len(), "loaded jobs");
    jobs
}

#[tracing::instrument(skip_all)]
pub fn spawn_scheduler(
    db: GhostDb,
    config: Config,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let tick_secs = config.timing.scheduler_tick_seconds;
        let workspace = config.workspace.clone();
        let jobs_dir = workspace.join("jobs");

        let mut job_map = build_job_map(&workspace);

        // Set up file watcher
        let (fs_tx, mut fs_rx) = mpsc::channel::<PathBuf>(64);
        let _watcher = match setup_jobs_watcher(&jobs_dir, fs_tx) {
            Ok(w) => Some(w),
            Err(e) => {
                logfire::warn!("failed to start jobs file watcher", error = e.to_string(),);
                None
            }
        };

        info!(
            tick_seconds = tick_secs,
            job_count = job_map.len(),
            "job scheduler started",
        );

        let mut interval = tokio::time::interval(Duration::from_secs(tick_secs));

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    tick(&db, &config, &mut job_map).await;
                }
                path = fs_rx.recv() => {
                    if let Some(_path) = path {
                        // Debounce: drain remaining events
                        tokio::time::sleep(Duration::from_millis(500)).await;
                        while fs_rx.try_recv().is_ok() {}

                        info!("jobs directory changed, reloading");
                        job_map = build_job_map(&workspace);
                    }
                }
                _ = shutdown.changed() => {
                    info!("job scheduler shutting down");
                    break;
                }
            }
        }

        info!("job scheduler stopped");
    })
}

fn build_job_map(workspace: &Path) -> HashMap<String, LoadedJob> {
    load_all_jobs(workspace)
        .into_iter()
        .map(|job| (job.definition.file_stem.clone(), job))
        .collect()
}

async fn tick(db: &GhostDb, config: &Config, jobs: &mut HashMap<String, LoadedJob>) {
    let now = Utc::now();

    for job in jobs.values_mut() {
        if !job.definition.enabled {
            continue;
        }

        let Some(next_run) = job.next_run else {
            continue;
        };

        if now < next_run {
            continue;
        }

        let job_name = job.definition.name.clone();
        logfire::info!("executing scheduled job", job_name = job_name.clone());

        match execute_job(db, config, &job.definition).await {
            Ok(()) => {
                logfire::info!("job completed", job_name = job_name,);
            }
            Err(e) => {
                logfire::error!("job failed", job_name = job_name, error = e.to_string(),);
            }
        }

        job.last_run = Some(now);
        job.next_run = next_run_after(&job.definition.schedule, &now);
    }
}

#[tracing::instrument(skip_all, fields(
    job_name = %def.name,
    model = %def.model,
))]
async fn execute_job(db: &GhostDb, config: &Config, def: &JobDefinition) -> Result<(), JobError> {
    let mut prompt = String::new();

    // Load carry_last_output state if enabled
    if def.carry_last_output {
        let state_path = config
            .workspace
            .join(".state")
            .join(format!("{}.last.md", def.file_stem));
        if state_path.exists() {
            match std::fs::read_to_string(&state_path) {
                Ok(previous) => {
                    prompt.push_str("## Previous output\n\n");
                    prompt.push_str(&previous);
                    prompt.push_str("\n\n---\n\n");
                }
                Err(e) => {
                    logfire::warn!(
                        "could not read state file",
                        path = state_path.display().to_string(),
                        error = e.to_string(),
                    );
                }
            }
        }
    }

    prompt.push_str(&def.prompt);

    // Resolve provider for the job's model alias
    let model_alias = if def.model == "default" {
        None
    } else {
        Some(def.model.as_str())
    };
    let provider = provider_for_alias(config, model_alias)?;

    let tool_manager = match def.tools {
        JobToolSet::Chat => ToolManager::for_chat(),
        JobToolSet::None => ToolManager::empty(),
    };

    let tool_set = match def.tools {
        JobToolSet::Chat => ToolSet::Chat,
        JobToolSet::None => ToolSet::Chat, // ToolSet is unused by chat_job currently
    };

    let session_chat = SessionChat::new(db.clone(), provider, tool_manager, config.clone());

    let session_id = crate::db::sessions::create_session(db).await?;
    let session_id_str = session_id.to_string();

    let transcript = session_chat
        .chat_job(&def.name, &session_id_str, &prompt, tool_set)
        .await?;

    // Save output for carry_last_output
    if def.carry_last_output {
        let state_dir = config.workspace.join(".state");
        if let Err(e) = std::fs::create_dir_all(&state_dir) {
            logfire::warn!("could not create .state directory", error = e.to_string(),);
        } else {
            let state_path = state_dir.join(format!("{}.last.md", def.file_stem));
            if let Err(e) = std::fs::write(&state_path, &transcript.result.message) {
                logfire::warn!(
                    "could not write state file",
                    path = state_path.display().to_string(),
                    error = e.to_string(),
                );
            }
        }
    }

    Ok(())
}

/// Run a single job by name (for `ghost job run <name>`).
#[tracing::instrument(skip_all, fields(job_name = %name))]
pub async fn run_job(db: &GhostDb, config: &Config, name: &str) -> Result<(), JobError> {
    let jobs = load_all_jobs(&config.workspace);
    let job = jobs
        .into_iter()
        .find(|j| j.definition.file_stem == name || j.definition.name == name)
        .ok_or_else(|| JobError::JobNotFound {
            name: name.to_string(),
        })?;

    execute_job(db, config, &job.definition).await
}

fn setup_jobs_watcher(
    jobs_dir: &Path,
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

    if jobs_dir.exists() {
        watcher.watch(jobs_dir, RecursiveMode::NonRecursive)?;
    }

    Ok(watcher)
}
