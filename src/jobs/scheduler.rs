use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;
use tracing::info;

use crate::agents::{TaskDefinition, TaskRunner};
use crate::config::Config;

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

/// Spawn the cron job scheduler. Watches the jobs directory for changes,
/// reloads job definitions on file events, and executes due jobs each tick.
#[tracing::instrument(skip_all)]
pub fn spawn_scheduler(
    task_runner: Arc<TaskRunner>,
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
                    tick(&task_runner, &config, &mut job_map).await;
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

async fn tick(task_runner: &TaskRunner, config: &Config, jobs: &mut HashMap<String, LoadedJob>) {
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

        match execute_job(task_runner, config, &job.definition).await {
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

/// Convert a `JobDefinition` into an `TaskDefinition` for execution.
fn job_to_task_definition(def: &JobDefinition) -> TaskDefinition {
    let tools = match def.tools {
        JobToolSet::Chat => vec![
            "run_shell_command",
            "read_file",
            "write_file",
            "file_edit",
            "todo",
            "knowledge_search",
            "web_search",
            "web_fetch",
        ]
        .into_iter()
        .map(String::from)
        .collect(),
        JobToolSet::None => vec![],
    };

    TaskDefinition {
        name: def.name.clone(),
        description: format!("Scheduled job: {}", def.name),
        tools,
        max_iterations: 25,
        model: if def.model == "default" {
            None
        } else {
            Some(def.model.clone())
        },
        progress_rules: vec![],
        skills: vec![],
        system_prompt_template: def.prompt.clone(),
        progress_gate: None,
        temporal: None,
        recency: None,
        context_pressure: None,
    }
}

#[tracing::instrument(name = "job", skip_all, fields(
    job_name = %def.name,
    model = %def.model,
))]
/// Execute a single job: optionally load previous output (carry_last_output),
/// convert the job definition into a task, run it to completion, and save output.
async fn execute_job(
    task_runner: &TaskRunner,
    config: &Config,
    def: &JobDefinition,
) -> Result<(), JobError> {
    let mut user_message = String::new();

    // Load carry_last_output state if enabled
    if def.carry_last_output {
        let state_path = config
            .workspace
            .join(".state")
            .join(format!("{}.last.md", def.file_stem));
        if state_path.exists() {
            match std::fs::read_to_string(&state_path) {
                Ok(previous) => {
                    user_message.push_str("## Previous output\n\n");
                    user_message.push_str(&previous);
                    user_message.push_str("\n\n---\n\n");
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

    user_message.push_str("Execute the scheduled job.");

    let task_def = job_to_task_definition(def);
    let response = task_runner
        .run_definition_to_completion(&task_def, &user_message, None)
        .await?;

    // Save output for carry_last_output
    if def.carry_last_output {
        let state_dir = config.workspace.join(".state");
        if let Err(e) = std::fs::create_dir_all(&state_dir) {
            logfire::warn!("could not create .state directory", error = e.to_string(),);
        } else {
            let state_path = state_dir.join(format!("{}.last.md", def.file_stem));
            if let Err(e) = std::fs::write(&state_path, &response) {
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
pub async fn run_job(
    task_runner: &TaskRunner,
    config: &Config,
    name: &str,
) -> Result<(), JobError> {
    let jobs = load_all_jobs(&config.workspace);
    let job = jobs
        .into_iter()
        .find(|j| j.definition.file_stem == name || j.definition.name == name)
        .ok_or_else(|| JobError::JobNotFound {
            name: name.to_string(),
        })?;

    execute_job(task_runner, config, &job.definition).await
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
