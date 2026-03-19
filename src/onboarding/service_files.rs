use super::OnboardingError;
use super::detect;
use crate::error::GhostError;

// ---------------------------------------------------------------------------
// Unit/plist generators
// ---------------------------------------------------------------------------

/// Generate a systemd user unit for the ghost daemon.
///
/// Includes `TimeoutStopSec=120` to give in-flight operations a chance to
/// finish before the service manager kills the process.
pub fn generate_daemon_unit_systemd(exe: &str, workspace: &str) -> String {
    format!(
        r#"[Unit]
Description=GHOST AI Agent Daemon
After=network-online.target

[Service]
ExecStart={exe} daemon
WorkingDirectory={workspace}
Environment=PATH=/nix/var/nix/profiles/default/bin:%h/.nix-profile/bin:/usr/local/bin:/usr/bin:/bin
Restart=always
RestartSec=2
TimeoutStopSec=120

[Install]
WantedBy=default.target
"#
    )
}

/// Generate a systemd user unit for the llama-server embedding service.
pub fn generate_llama_server_unit_systemd(exe: &str, model: &str) -> String {
    format!(
        r#"[Unit]
Description=llama-server embedding service
After=network-online.target

[Service]
ExecStart={exe} --embedding --model {model} --port 11434
Restart=on-failure
RestartSec=5
TimeoutStopSec=30
Environment=PATH=/nix/var/nix/profiles/default/bin:%h/.nix-profile/bin:/usr/local/bin:/usr/bin:/bin

[Install]
WantedBy=default.target
"#
    )
}

/// Generate a systemd user unit for the docling-serve document processing service.
pub fn generate_docling_unit_systemd(exe: &str) -> String {
    format!(
        r#"[Unit]
Description=docling-serve document processing
After=network-online.target

[Service]
ExecStart={exe}
Restart=on-failure
RestartSec=5
TimeoutStopSec=30
Environment=PATH=/nix/var/nix/profiles/default/bin:%h/.nix-profile/bin:/usr/local/bin:/usr/bin:/bin

[Install]
WantedBy=default.target
"#
    )
}

/// Generate a launchd plist for the ghost daemon.
pub fn generate_daemon_plist(exe: &str, workspace: &str) -> String {
    let log_dir = dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .join("ghost/logs");
    let log_dir = log_dir.display().to_string();
    let _ = workspace; // workspace stored in config, not needed in plist
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
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
"#
    )
}

/// Generate a launchd plist for the llama-server embedding service.
pub fn generate_llama_server_plist(exe: &str, model: &str) -> String {
    let log_dir = dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .join("ghost/logs");
    let log_dir = log_dir.display().to_string();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>com.ghost.llama-server</string>
  <key>ProgramArguments</key>
  <array>
    <string>{exe}</string>
    <string>--embedding</string>
    <string>--model</string>
    <string>{model}</string>
    <string>--port</string>
    <string>11434</string>
  </array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>StandardOutPath</key>
  <string>{log_dir}/llama-server.log</string>
  <key>StandardErrorPath</key>
  <string>{log_dir}/llama-server.err</string>
</dict>
</plist>
"#
    )
}

/// Generate a launchd plist for the docling-serve document processing service.
pub fn generate_docling_plist(exe: &str) -> String {
    let log_dir = dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .join("ghost/logs");
    let log_dir = log_dir.display().to_string();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>com.ghost.docling-serve</string>
  <key>ProgramArguments</key>
  <array>
    <string>{exe}</string>
  </array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>StandardOutPath</key>
  <string>{log_dir}/docling-serve.log</string>
  <key>StandardErrorPath</key>
  <string>{log_dir}/docling-serve.err</string>
</dict>
</plist>
"#
    )
}

// ---------------------------------------------------------------------------
// Path helpers
// ---------------------------------------------------------------------------

/// Resolve the ghost binary path for service files.
///
/// Prefers the first PATH entry for the binary name over the fully-resolved
/// `current_exe()`. On nix, `current_exe()` resolves through profile symlinks to
/// a volatile `/nix/store/<hash>/bin/ghost` path — using the PATH entry
/// (e.g. `~/.nix-profile/bin/ghost`) ensures the service file survives upgrades.
pub fn stable_exe_path() -> Result<String, GhostError> {
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

/// Check if `loginctl enable-linger` is set for the current user and enable it
/// if not.
///
/// Without linger, systemd kills all user services when the last login session
/// ends, which causes the daemon to die whenever an SSH session disconnects.
pub fn ensure_linger_enabled() {
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

// ---------------------------------------------------------------------------
// Single-service installer (used by `ghost update` and legacy init path)
// ---------------------------------------------------------------------------

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

        let content = generate_daemon_plist(&exe, &config.workspace.display().to_string());
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

        let content = generate_daemon_unit_systemd(&exe, &config.workspace.display().to_string());
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

// ---------------------------------------------------------------------------
// Bulk installer (wizard phase)
// ---------------------------------------------------------------------------

/// Install all applicable service files and return the list of paths written.
///
/// Always installs the ghost daemon unit. Installs llama-server and docling
/// units when executables are provided. On Linux, enables systemd linger.
pub fn install_all_service_files(
    platform: &detect::Platform,
    exe: &str,
    workspace: &str,
    llama_server_exe: Option<&str>,
    docling_exe: Option<&str>,
) -> Result<Vec<String>, OnboardingError> {
    let mut written = Vec::new();

    match platform {
        detect::Platform::MacOs => {
            let plist_dir = dirs::home_dir()
                .ok_or_else(|| std::io::Error::other("cannot determine home directory"))?
                .join("Library/LaunchAgents");
            std::fs::create_dir_all(&plist_dir)?;

            let log_dir = dirs::data_dir()
                .ok_or_else(|| std::io::Error::other("cannot determine data directory"))?
                .join("ghost/logs");
            std::fs::create_dir_all(&log_dir)?;

            // Ghost daemon
            let path = plist_dir.join("com.ghost.daemon.plist");
            std::fs::write(&path, generate_daemon_plist(exe, workspace))?;
            written.push(path.display().to_string());

            // llama-server
            if let Some(ls_exe) = llama_server_exe {
                let model = "qwen3-embedding:8b";
                let path = plist_dir.join("com.ghost.llama-server.plist");
                std::fs::write(&path, generate_llama_server_plist(ls_exe, model))?;
                written.push(path.display().to_string());
            }

            // docling-serve
            if let Some(dl_exe) = docling_exe {
                let path = plist_dir.join("com.ghost.docling-serve.plist");
                std::fs::write(&path, generate_docling_plist(dl_exe))?;
                written.push(path.display().to_string());
            }
        }
        detect::Platform::Linux | detect::Platform::Other(_) => {
            let unit_dir = dirs::config_dir()
                .ok_or_else(|| std::io::Error::other("cannot determine config directory"))?
                .join("systemd/user");
            std::fs::create_dir_all(&unit_dir)?;

            // Ghost daemon
            let path = unit_dir.join("ghost-daemon.service");
            std::fs::write(&path, generate_daemon_unit_systemd(exe, workspace))?;
            written.push(path.display().to_string());

            // llama-server
            if let Some(ls_exe) = llama_server_exe {
                let model = "qwen3-embedding:8b";
                let path = unit_dir.join("llama-server.service");
                std::fs::write(&path, generate_llama_server_unit_systemd(ls_exe, model))?;
                written.push(path.display().to_string());
            }

            // docling-serve
            if let Some(dl_exe) = docling_exe {
                let path = unit_dir.join("docling-serve.service");
                std::fs::write(&path, generate_docling_unit_systemd(dl_exe))?;
                written.push(path.display().to_string());
            }

            ensure_linger_enabled();
        }
    }

    Ok(written)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daemon_unit_has_timeout() {
        let unit = generate_daemon_unit_systemd("/usr/bin/ghost", "/home/user/GHOST");
        assert!(unit.contains("TimeoutStopSec=120"));
        assert!(unit.contains("ExecStart=/usr/bin/ghost daemon"));
        assert!(unit.contains("WorkingDirectory=/home/user/GHOST"));
    }

    #[test]
    fn llama_server_unit() {
        let unit = generate_llama_server_unit_systemd(
            "/home/user/.nix-profile/bin/llama-server",
            "qwen3-embedding:8b",
        );
        assert!(unit.contains("llama-server"));
        assert!(unit.contains("--embedding"));
        assert!(unit.contains("qwen3-embedding:8b"));
    }

    #[test]
    fn docling_unit() {
        let unit = generate_docling_unit_systemd("/home/user/.nix-profile/bin/docling-serve");
        assert!(unit.contains("docling-serve"));
        assert!(unit.contains("Restart=on-failure"));
    }

    #[test]
    fn daemon_plist_has_keep_alive() {
        let plist = generate_daemon_plist("/usr/bin/ghost", "/Users/user/GHOST");
        assert!(plist.contains("<key>KeepAlive</key>"));
        assert!(plist.contains("<true/>"));
        assert!(plist.contains("/usr/bin/ghost"));
    }
}
