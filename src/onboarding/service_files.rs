use super::OnboardingError;
use super::detect;
use crate::error::GhostError;

// ---------------------------------------------------------------------------
// Unit/plist generators
// ---------------------------------------------------------------------------

/// Generate a systemd unit for the ghost daemon.
///
/// Includes `TimeoutStopSec=120` to give in-flight operations a chance to
/// finish before the service manager kills the process.
///
/// When `system_level` is true (running as root), the actual home path is
/// substituted for `%h` (which is not supported in system units) and
/// `WantedBy` is set to `multi-user.target`.
pub fn generate_daemon_unit_systemd(exe: &str, workspace: &str, system_level: bool) -> String {
    let (home, target) = if system_level {
        let home = dirs::home_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("/root"))
            .display()
            .to_string();
        (home, "multi-user.target")
    } else {
        ("%h".to_string(), "default.target")
    };

    format!(
        r#"[Unit]
Description=GHOST AI Agent Daemon
After=network-online.target

[Service]
ExecStart={exe} daemon
WorkingDirectory={workspace}
Environment=PATH=/nix/var/nix/profiles/default/bin:{home}/.nix-profile/bin:/usr/local/bin:/usr/bin:/bin
Restart=always
RestartSec=2
TimeoutStopSec=120

[Install]
WantedBy={target}
"#
    )
}

/// Generate a systemd unit for the llama-server embedding service.
///
/// When `system_level` is true (running as root), the actual home path is
/// substituted for `%h` (which is not supported in system units) and
/// `WantedBy` is set to `multi-user.target`.
pub fn generate_llama_server_unit_systemd(
    exe: &str,
    hf_repo: &str,
    alias: &str,
    system_level: bool,
) -> String {
    let (home, target) = if system_level {
        let home = dirs::home_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("/root"))
            .display()
            .to_string();
        (home, "multi-user.target")
    } else {
        ("%h".to_string(), "default.target")
    };

    format!(
        r#"[Unit]
Description=llama-server embedding service
After=network-online.target

[Service]
ExecStart={exe} --embedding --hf-repo {hf_repo} --alias {alias} --port 11434
Restart=on-failure
RestartSec=5
TimeoutStopSec=30
Environment=PATH=/nix/var/nix/profiles/default/bin:{home}/.nix-profile/bin:/usr/local/bin:/usr/bin:/bin

[Install]
WantedBy={target}
"#
    )
}

/// Generate a launchd plist for the ghost daemon.
pub fn generate_daemon_plist(exe: &str, workspace: &str) -> String {
    let log_dir = dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .join("ghost/logs");
    let log_dir = log_dir.display().to_string();
    let home = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .display()
        .to_string();
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
  <key>EnvironmentVariables</key>
  <dict>
    <key>PATH</key>
    <string>/nix/var/nix/profiles/default/bin:{home}/.nix-profile/bin:/usr/local/bin:/usr/bin:/bin</string>
  </dict>
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
pub fn generate_llama_server_plist(exe: &str, hf_repo: &str, alias: &str) -> String {
    let log_dir = dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .join("ghost/logs");
    let log_dir = log_dir.display().to_string();
    let home = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .display()
        .to_string();
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
    <string>--hf-repo</string>
    <string>{hf_repo}</string>
    <string>--alias</string>
    <string>{alias}</string>
    <string>--port</string>
    <string>11434</string>
  </array>
  <key>EnvironmentVariables</key>
  <dict>
    <key>PATH</key>
    <string>/nix/var/nix/profiles/default/bin:{home}/.nix-profile/bin:/usr/local/bin:/usr/bin:/bin</string>
  </dict>
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
// Bulk installer (wizard phase)
// ---------------------------------------------------------------------------

/// llama-server installation info: executable path, HF repo, and API alias.
pub struct LlamaServerInfo<'a> {
    pub exe: &'a str,
    pub hf_repo: &'a str,
    pub alias: &'a str,
}

/// Install all applicable service files and return the list of paths written.
///
/// Always installs the ghost daemon unit. Installs llama-server unit when
/// provided. On Linux, enables systemd linger.
pub fn install_all_service_files(
    platform: &detect::Platform,
    exe: &str,
    workspace: &str,
    llama_server: Option<&LlamaServerInfo<'_>>,
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
            if let Some(ls) = llama_server {
                let path = plist_dir.join("com.ghost.llama-server.plist");
                std::fs::write(
                    &path,
                    generate_llama_server_plist(ls.exe, ls.hf_repo, ls.alias),
                )?;
                written.push(path.display().to_string());
            }
        }
        detect::Platform::Linux | detect::Platform::Other(_) => {
            let unit_dir = crate::systemd::unit_dir()?;
            std::fs::create_dir_all(&unit_dir)?;

            // Ghost daemon
            let path = unit_dir.join("ghost-daemon.service");
            std::fs::write(
                &path,
                generate_daemon_unit_systemd(exe, workspace, crate::systemd::is_root()),
            )?;
            written.push(path.display().to_string());

            // llama-server
            if let Some(ls) = llama_server {
                let path = unit_dir.join("llama-server.service");
                std::fs::write(
                    &path,
                    generate_llama_server_unit_systemd(
                        ls.exe,
                        ls.hf_repo,
                        ls.alias,
                        crate::systemd::is_root(),
                    ),
                )?;
                written.push(path.display().to_string());
            }

            if !crate::systemd::is_root() {
                ensure_linger_enabled();
            }
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
        let unit = generate_daemon_unit_systemd("/usr/bin/ghost", "/home/user/GHOST", false);
        assert!(unit.contains("TimeoutStopSec=120"));
        assert!(unit.contains("ExecStart=/usr/bin/ghost daemon"));
        assert!(unit.contains("WorkingDirectory=/home/user/GHOST"));
    }

    #[test]
    fn llama_server_unit() {
        let unit = generate_llama_server_unit_systemd(
            "/home/user/.nix-profile/bin/llama-server",
            "Qwen/Qwen3-Embedding-8B-GGUF:Q8_0",
            "qwen3-embedding:8b",
            false,
        );
        assert!(unit.contains("llama-server"));
        assert!(unit.contains("--embedding"));
        assert!(unit.contains("--hf-repo Qwen/Qwen3-Embedding-8B-GGUF:Q8_0"));
        assert!(unit.contains("--alias qwen3-embedding:8b"));
    }

    #[test]
    fn daemon_unit_system_level() {
        let unit = generate_daemon_unit_systemd("/usr/bin/ghost", "/home/user/GHOST", true);
        assert!(unit.contains("WantedBy=multi-user.target"));
        assert!(!unit.contains("%h"));
    }

    #[test]
    fn llama_server_unit_system_level() {
        let unit = generate_llama_server_unit_systemd(
            "/usr/bin/llama-server",
            "Qwen/Qwen3-Embedding-8B-GGUF:Q8_0",
            "qwen3-embedding:8b",
            true,
        );
        assert!(unit.contains("WantedBy=multi-user.target"));
        assert!(!unit.contains("%h"));
    }

    #[test]
    fn daemon_plist_has_keep_alive() {
        let plist = generate_daemon_plist("/usr/bin/ghost", "/Users/user/GHOST");
        assert!(plist.contains("<key>KeepAlive</key>"));
        assert!(plist.contains("<true/>"));
        assert!(plist.contains("/usr/bin/ghost"));
    }
}
