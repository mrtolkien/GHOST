use crate::error::GhostError;

/// Read the daemon PID file and send SIGTERM to trigger a graceful restart.
pub fn execute() -> Result<(), GhostError> {
    let config = crate::config::load()?;
    let pid = crate::daemon::pidfile::read_pidfile(&config.workspace).map_err(|e| {
        std::io::Error::new(
            e.kind(),
            format!(
                "could not read PID file at {}: {e} — is the daemon running?",
                crate::daemon::pidfile::pidfile_path(&config.workspace).display()
            ),
        )
    })?;

    let status = std::process::Command::new("kill")
        .arg(pid.to_string())
        .status()
        .map_err(|e| std::io::Error::new(e.kind(), format!("failed to run kill: {e}")))?;

    if !status.success() {
        return Err(std::io::Error::other(format!(
            "kill exited with status {status} for PID {pid}"
        ))
        .into());
    }

    println!("sent SIGTERM to ghost daemon (PID {pid})");
    Ok(())
}
