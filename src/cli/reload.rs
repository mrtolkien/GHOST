use crate::error::GhostError;

/// Validate the current config and signal the running daemon to reload.
pub fn execute() -> Result<(), GhostError> {
    // Step 1: Validate the new config
    match crate::config::load() {
        Ok(_) => {}
        Err(e) => {
            eprintln!("Config validation failed:\n  {e}");
            std::process::exit(1);
        }
    }

    // Step 2: Send SIGHUP via service manager
    if cfg!(target_os = "macos") {
        let uid = std::process::Command::new("id")
            .arg("-u")
            .output()
            .map_err(|e| std::io::Error::new(e.kind(), format!("failed to run id: {e}")))?;
        let uid = String::from_utf8_lossy(&uid.stdout).trim().to_string();

        let status = std::process::Command::new("launchctl")
            .args(["kill", "SIGHUP", &format!("gui/{uid}/com.ghost.daemon")])
            .status()
            .map_err(|e| std::io::Error::new(e.kind(), format!("failed to run launchctl: {e}")))?;

        if !status.success() {
            return Err(std::io::Error::other(
                "launchctl kill SIGHUP failed — is the daemon running?",
            )
            .into());
        }
    } else {
        let status = std::process::Command::new("systemctl")
            .args(["--user", "kill", "--signal=SIGHUP", "ghost-daemon"])
            .status()
            .map_err(|e| std::io::Error::new(e.kind(), format!("failed to run systemctl: {e}")))?;

        if !status.success() {
            return Err(std::io::Error::other(
                "systemctl kill SIGHUP failed — is the daemon running?",
            )
            .into());
        }
    }

    println!("Config reloaded successfully.");
    Ok(())
}
