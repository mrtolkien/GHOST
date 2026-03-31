use std::fs;
use std::path::{Path, PathBuf};

use tracing::info;

const PID_FILE_NAME: &str = ".ghost.pid";

#[derive(Debug, thiserror::Error)]
pub enum PidFileError {
    #[error(
        "another GHOST daemon is already running (PID {pid}).\n\
         If this is stale, remove {path}"
    )]
    AlreadyRunning { pid: u32, path: PathBuf },

    #[error("failed to write PID file {path}: {source}")]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("failed to read PID file {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
}

/// Path to the PID file inside the workspace.
pub fn pid_file_path(workspace: &Path) -> PathBuf {
    workspace.join(PID_FILE_NAME)
}

/// Acquire the PID file lock. Returns an error if another daemon is alive.
pub fn acquire(workspace: &Path) -> Result<(), PidFileError> {
    let path = pid_file_path(workspace);

    if path.exists() {
        let contents = fs::read_to_string(&path).map_err(|source| PidFileError::Read {
            path: path.clone(),
            source,
        })?;

        if let Ok(old_pid) = contents.trim().parse::<u32>() {
            if is_process_alive(old_pid) {
                return Err(PidFileError::AlreadyRunning { pid: old_pid, path });
            }
            info!(old_pid, "removing stale PID file");
        }
    }

    let pid = std::process::id();
    fs::write(&path, pid.to_string()).map_err(|source| PidFileError::Write { path, source })?;

    info!(pid, "PID file acquired");
    Ok(())
}

/// Remove the PID file on shutdown (best-effort).
pub fn release(workspace: &Path) {
    let path = pid_file_path(workspace);
    // Only remove if it's our own PID (guard against races)
    if let Ok(contents) = fs::read_to_string(&path) {
        if contents.trim().parse::<u32>().ok() == Some(std::process::id()) {
            let _ = fs::remove_file(&path);
        }
    }
}

/// Check if a process with the given PID is still running.
fn is_process_alive(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}
