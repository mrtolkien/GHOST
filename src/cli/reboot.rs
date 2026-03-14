use crate::error::GhostError;

/// Restart the ghost daemon via systemd/launchd.
pub fn execute() -> Result<(), GhostError> {
    if cfg!(target_os = "macos") {
        let uid = std::process::Command::new("id")
            .arg("-u")
            .output()
            .map_err(|e| std::io::Error::new(e.kind(), format!("failed to run id: {e}")))?;
        let uid = String::from_utf8_lossy(&uid.stdout).trim().to_string();

        let status = std::process::Command::new("launchctl")
            .args(["kickstart", "-k", &format!("gui/{uid}/com.ghost.daemon")])
            .status()
            .map_err(|e| std::io::Error::new(e.kind(), format!("failed to run launchctl: {e}")))?;

        if !status.success() {
            return Err(std::io::Error::other("launchctl kickstart failed").into());
        }
        println!("restarting ghost daemon via launchctl");
    } else {
        // Reload unit files in case the service file was regenerated
        let _ = std::process::Command::new("systemctl")
            .args(["--user", "daemon-reload"])
            .status();

        let status = std::process::Command::new("systemctl")
            .args(["--user", "restart", "ghost-daemon"])
            .status()
            .map_err(|e| std::io::Error::new(e.kind(), format!("failed to run systemctl: {e}")))?;

        if !status.success() {
            return Err(std::io::Error::other("systemctl restart ghost-daemon failed").into());
        }
        println!("restarting ghost daemon via systemctl");
    }

    Ok(())
}
