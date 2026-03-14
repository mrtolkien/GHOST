use crate::error::GhostError;

const SYSTEMD_UNIT: &str = r#"[Unit]
Description=GHOST AI Agent Daemon
After=network-online.target

[Service]
ExecStart={exe} daemon
WorkingDirectory={workspace}
Environment=PATH=/nix/var/nix/profiles/default/bin:%h/.nix-profile/bin:/usr/local/bin:/usr/bin:/bin
Restart=always
RestartSec=2

[Install]
WantedBy=default.target
"#;

const LAUNCHD_PLIST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>com.ghost.daemon</string>
  <key>ProgramArguments</key>
  <array>
    <string>{exe}</string>
    <string>daemon</string>
  </array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>StandardOutPath</key>
  <string>{log_dir}/ghost-daemon.log</string>
  <key>StandardErrorPath</key>
  <string>{log_dir}/ghost-daemon.err</string>
</dict>
</plist>
"#;

#[tracing::instrument(skip_all)]
pub async fn execute() -> Result<(), GhostError> {
    let config = crate::config::load()?;
    crate::config_workspace::bootstrap_workspace(&config)?;

    install_service_file(&config, false)?;

    Ok(())
}

/// Resolve the ghost binary path for service files.
///
/// Prefers the first PATH entry for the binary name over the fully-resolved
/// `current_exe()`. On nix, `current_exe()` resolves through profile symlinks to
/// a volatile `/nix/store/<hash>/bin/ghost` path — using the PATH entry
/// (e.g. `~/.nix-profile/bin/ghost`) ensures the service file survives upgrades.
fn stable_exe_path() -> Result<String, GhostError> {
    let resolved = std::env::current_exe()
        .map_err(|e| std::io::Error::new(e.kind(), format!("cannot find own binary: {e}")))?;

    let exe_name = resolved
        .file_name()
        .ok_or_else(|| std::io::Error::other("cannot determine binary name"))?;

    // Find the binary in PATH without resolving symlinks
    if let Some(path_var) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let candidate = dir.join(exe_name);
            if candidate.exists() {
                return Ok(candidate.display().to_string());
            }
        }
    }

    Ok(resolved.display().to_string())
}

/// Check if `loginctl enable-linger` is set for the current user.
/// Without linger, systemd kills all user services when the last login session ends,
/// which causes the daemon to die whenever an SSH session disconnects.
fn ensure_linger_enabled() {
    let user = std::env::var("USER").unwrap_or_default();
    if user.is_empty() {
        return;
    }

    // Check current linger status
    let output = std::process::Command::new("loginctl")
        .args(["show-user", &user, "--property=Linger"])
        .output();

    let already_enabled = output
        .as_ref()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout.clone()).ok())
        .is_some_and(|s| s.trim() == "Linger=yes");

    if already_enabled {
        return;
    }

    // Try to enable it
    println!();
    println!("enabling loginctl linger for user '{user}'...");
    let result = std::process::Command::new("loginctl")
        .args(["enable-linger", &user])
        .status();

    match result {
        Ok(s) if s.success() => {
            println!("linger enabled — daemon will persist after logout");
        }
        _ => {
            println!(
                "warning: could not enable linger. Run manually:\n  \
                 sudo loginctl enable-linger {user}\n\n\
                 Without linger, the daemon will stop when you log out."
            );
        }
    }
}

/// Write the daemon service file (systemd unit or launchd plist).
///
/// `quiet`: skip printing start instructions (used during `ghost update`).
pub(crate) fn install_service_file(
    config: &crate::config::Config,
    quiet: bool,
) -> Result<(), GhostError> {
    let exe = stable_exe_path()?;

    if cfg!(target_os = "macos") {
        let plist_dir = dirs::home_dir()
            .ok_or_else(|| std::io::Error::other("cannot determine home directory"))?
            .join("Library/LaunchAgents");
        std::fs::create_dir_all(&plist_dir)?;

        let log_dir = dirs::data_dir()
            .ok_or_else(|| std::io::Error::other("cannot determine data directory"))?
            .join("ghost/logs");
        std::fs::create_dir_all(&log_dir)?;

        let content = LAUNCHD_PLIST
            .replace("{exe}", &exe)
            .replace("{log_dir}", &log_dir.display().to_string());

        let plist_path = plist_dir.join("com.ghost.daemon.plist");
        std::fs::write(&plist_path, content)?;

        if !quiet {
            println!("service file written to {}", plist_path.display());
            println!();
            println!("start the daemon with:");
            println!(
                "  launchctl bootstrap gui/$(id -u) {}",
                plist_path.display()
            );
        }
    } else {
        // Linux — systemd user unit
        let unit_dir = dirs::config_dir()
            .ok_or_else(|| std::io::Error::other("cannot determine config directory"))?
            .join("systemd/user");
        std::fs::create_dir_all(&unit_dir)?;

        let content = SYSTEMD_UNIT
            .replace("{exe}", &exe)
            .replace("{workspace}", &config.workspace.display().to_string());
        let unit_path = unit_dir.join("ghost-daemon.service");
        std::fs::write(&unit_path, content)?;

        if !quiet {
            println!("service file written to {}", unit_path.display());
            println!();
            println!("start the daemon with:");
            println!("  systemctl --user enable --now ghost-daemon");

            ensure_linger_enabled();
        }
    }

    Ok(())
}
