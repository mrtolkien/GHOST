use std::path::{Path, PathBuf};

/// Returns the PID file path: `<workspace>/.ghost.pid`.
pub fn pidfile_path(workspace: &Path) -> PathBuf {
    workspace.join(".ghost.pid")
}

/// Write the current process PID to the workspace PID file.
pub fn write_pidfile(workspace: &Path) -> std::io::Result<()> {
    std::fs::write(pidfile_path(workspace), std::process::id().to_string())
}

/// Remove the PID file if it exists.
pub fn remove_pidfile(workspace: &Path) {
    let _ = std::fs::remove_file(pidfile_path(workspace));
}

/// Read the PID from the workspace PID file.
pub fn read_pidfile(workspace: &Path) -> std::io::Result<u32> {
    let contents = std::fs::read_to_string(pidfile_path(workspace))?;
    contents
        .trim()
        .parse::<u32>()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}
