use std::path::{Path, PathBuf};

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TablePolicy {
    FileBackedRebuild,
    DbOnlyCopy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TableVerification {
    pub table: &'static str,
    pub copied_rows: u64,
    pub source_rows: u64,
    pub verified: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct RepairReport {
    pub timestamp: String,
    pub candidate_db: PathBuf,
    pub live_db: PathBuf,
    pub backup_db: Option<PathBuf>,
    pub tables: Vec<TableVerification>,
    pub success: bool,
    pub failure_reason: Option<String>,
}

pub fn candidate_db_path(workspace_db: &Path, stamp: &str) -> PathBuf {
    repair_artifact_path(workspace_db, stamp, "candidate")
}

pub fn report_path(workspace_db: &Path, stamp: &str) -> PathBuf {
    repair_artifact_path(workspace_db, stamp, "report.json")
}

pub fn backup_path(workspace_db: &Path, stamp: &str) -> PathBuf {
    let file_name = workspace_db
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();

    workspace_db.with_file_name(format!("{file_name}.backup-{stamp}"))
}

fn repair_artifact_path(workspace_db: &Path, stamp: &str, suffix: &str) -> PathBuf {
    let file_name = workspace_db
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();

    workspace_db.with_file_name(format!("{file_name}.repair-{stamp}.{suffix}"))
}
