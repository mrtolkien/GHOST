use std::path::PathBuf;

use crate::config::Config;
use crate::db;
use crate::db::GhostDb;

pub struct CodingSession {
    pub id: String,
    pub session_id: String,
    pub working_dir: PathBuf,
    pub channel_id: Option<String>,
}

/// Start a new coding session. Creates a chat session, registers the
/// coding session in the DB, and returns the IDs needed for takeover.
pub async fn start(
    db: &GhostDb,
    _config: &Config,
    working_dir: PathBuf,
    channel_id: Option<String>,
    prompt: Option<String>,
) -> Result<CodingSession, CodingError> {
    let session_id = db::sessions::create_session(db).await?;
    let coding_id = ulid::Ulid::new().to_string();

    db::coding_sessions::create_coding_session(
        db,
        &coding_id,
        &session_id,
        channel_id.as_deref(),
        &working_dir.display().to_string(),
    )
    .await?;

    if let Some(prompt) = prompt {
        db::sessions::create_message(db, &session_id, "user", &prompt).await?;
    }

    Ok(CodingSession {
        id: coding_id,
        session_id,
        working_dir,
        channel_id,
    })
}

/// End a coding session. Generates deterministic summary from git state.
pub async fn end(
    db: &GhostDb,
    coding_session_id: &str,
    working_dir: &std::path::Path,
) -> Result<String, CodingError> {
    db::coding_sessions::end_coding_session(db, coding_session_id).await?;
    generate_summary(working_dir).await
}

/// Generate a deterministic summary from git log + diff --stat.
async fn generate_summary(working_dir: &std::path::Path) -> Result<String, CodingError> {
    let branch = run_git(working_dir, &["rev-parse", "--abbrev-ref", "HEAD"]).await?;

    let log = run_git(working_dir, &["log", "--oneline", "-20", "--no-decorate"])
        .await
        .unwrap_or_default();

    let stat = run_git(working_dir, &["diff", "--stat", "HEAD~20..HEAD"])
        .await
        .unwrap_or_default();

    let mut summary = String::new();
    summary.push_str(&format!("Branch: {branch}\n"));
    if !log.is_empty() {
        summary.push_str(&format!("\nCommits:\n{log}\n"));
    }
    if !stat.is_empty() {
        summary.push_str(&format!("\nChanged:\n{stat}\n"));
    }
    Ok(summary)
}

async fn run_git(dir: &std::path::Path, args: &[&str]) -> Result<String, CodingError> {
    let output = tokio::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .await
        .map_err(|e| CodingError::Git(e.to_string()))?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[derive(Debug, thiserror::Error)]
pub enum CodingError {
    #[error("database error: {0}")]
    Database(#[from] db::DatabaseError),
    #[error("git error: {0}")]
    Git(String),
    #[error("working directory not found: {0}")]
    WorkingDirNotFound(String),
    #[error("coding session is already active: {0}")]
    SessionAlreadyActive(String),
}
