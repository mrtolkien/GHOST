use clap::Subcommand;

use crate::error::GhostError;
use crate::jobs::{self, parse_job_file};

#[derive(Debug, Subcommand)]
pub enum JobCommand {
    List,
    Validate { path: String },
    Run { name: String },
    Logs { name: Option<String> },
}

#[tracing::instrument(skip_all)]
pub async fn execute(command: JobCommand) -> Result<(), GhostError> {
    match command {
        JobCommand::Validate { path } => cmd_validate(&path),
        JobCommand::List => cmd_list().await,
        JobCommand::Run { name } => cmd_run(&name).await,
        JobCommand::Logs { name } => cmd_logs(name.as_deref()).await,
    }
}

fn cmd_validate(path: &str) -> Result<(), GhostError> {
    let file_path = std::path::Path::new(path);
    if !file_path.exists() {
        return Err(jobs::JobError::FileNotFound {
            path: file_path.to_path_buf(),
        }
        .into());
    }

    let content = std::fs::read_to_string(file_path).map_err(|source| jobs::JobError::Io {
        path: file_path.to_path_buf(),
        source,
    })?;

    let def = parse_job_file(file_path, &content)?;

    println!("Valid job file:");
    println!("  Name:              {}", def.name);
    println!("  Enabled:           {}", def.enabled);
    println!("  Schedule:          {}", def.schedule_raw);
    println!("  Model:             {}", def.model);
    println!("  Tools:             {:?}", def.tools);
    println!("  Carry last output: {}", def.carry_last_output);

    if let Some(next) = jobs::next_run_after(&def.schedule, &chrono::Utc::now()) {
        println!("  Next run:          {}", next.format("%Y-%m-%d %H:%M UTC"));
    }

    Ok(())
}

async fn cmd_list() -> Result<(), GhostError> {
    let config = crate::config::load()?;
    let loaded = jobs::load_all_jobs(&config.workspace);

    if loaded.is_empty() {
        println!("No job files found in {}/jobs/", config.workspace.display());
        return Ok(());
    }

    println!(
        "{:<20} {:<8} {:<20} {:<10} NEXT RUN",
        "NAME", "ENABLED", "SCHEDULE", "MODEL"
    );
    println!("{}", "-".repeat(78));

    for job in &loaded {
        let d = &job.definition;
        let next_run = job
            .next_run
            .map(|t| t.format("%Y-%m-%d %H:%M UTC").to_string())
            .unwrap_or_else(|| "-".to_string());

        println!(
            "{:<20} {:<8} {:<20} {:<10} {}",
            d.name, d.enabled, d.schedule_raw, d.model, next_run,
        );
    }

    Ok(())
}

async fn cmd_run(name: &str) -> Result<(), GhostError> {
    let config = crate::config::load()?;
    crate::config_workspace::bootstrap_workspace(&config)?;
    let db = crate::db::connect(&config.workspace).await?;
    let task_runner = crate::agents::TaskRunner::new(db, config.clone());

    println!("Running job '{name}'...");
    jobs::run_job(&task_runner, &config, name).await?;
    println!("Job '{name}' completed.");
    Ok(())
}

async fn cmd_logs(name: Option<&str>) -> Result<(), GhostError> {
    let config = crate::config::load()?;
    let db = crate::db::connect(&config.workspace).await?;

    let logs = crate::db::job_logs::list_job_logs(&db, name, 20).await?;

    if logs.is_empty() {
        println!("No job logs found.");
        return Ok(());
    }

    println!(
        "{:<20} {:<8} {:<8} {:<20} DURATION",
        "JOB", "KIND", "STATUS", "STARTED"
    );
    println!("{}", "-".repeat(76));

    for log in &logs {
        let started = log.started_at.to_string();
        let started_short = started.get(..19).unwrap_or(&started);

        let duration = log
            .finished_at
            .as_ref()
            .map(|f| {
                let start: chrono::DateTime<chrono::Utc> = log.started_at.0;
                let end: chrono::DateTime<chrono::Utc> = f.0;
                let dur = end - start;
                format!("{}s", dur.num_seconds())
            })
            .unwrap_or_else(|| "running".to_string());

        println!(
            "{:<20} {:<8} {:<8} {:<20} {}",
            log.job_name, log.job_kind, log.status, started_short, duration,
        );
    }

    Ok(())
}
