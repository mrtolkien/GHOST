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

    install_service_file(&config)?;

    Ok(())
}

fn install_service_file(config: &crate::config::Config) -> Result<(), GhostError> {
    let exe = std::env::current_exe()
        .map_err(|e| std::io::Error::new(e.kind(), format!("cannot find own binary: {e}")))?
        .display()
        .to_string();

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

        println!("service file written to {}", plist_path.display());
        println!();
        println!("start the daemon with:");
        println!(
            "  launchctl bootstrap gui/$(id -u) {}",
            plist_path.display()
        );
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

        println!("service file written to {}", unit_path.display());
        println!();
        println!("start the daemon with:");
        println!("  systemctl --user enable --now ghost-daemon");
    }

    Ok(())
}
