#[cfg(target_os = "macos")]
use std::process::Command;

use crate::error::GhostError;
use crate::services::{ServiceField, ServiceRegistry};

/// Start all registered services (top-to-bottom, stop on first failure),
/// then start the daemon, then print status.
pub async fn execute_start() -> Result<(), GhostError> {
    let path = super::services::services_toml_path()?;
    let registry = ServiceRegistry::load_or_empty(&path)?;

    if !registry.entries.is_empty() {
        println!("Starting services…");
    }

    let results = registry.run_field(ServiceField::Start, true, false);

    if !results.is_empty() {
        for r in &results {
            if r.success {
                println!("  ✓ {}", r.service);
            } else {
                eprintln!("  ✗ {}: {}", r.service, r.output);
            }
        }

        if results.iter().any(|r| !r.success) {
            return Err(GhostError::Other(
                "one or more services failed to start — daemon not started".into(),
            ));
        }
    }

    println!("Starting daemon…");
    start_daemon()?;

    crate::cli::status::execute().await
}

/// Stop the daemon, then stop all registered services (bottom-to-top, best-effort),
/// then print status.
pub async fn execute_stop() -> Result<(), GhostError> {
    println!("Stopping daemon…");
    stop_daemon();

    let path = super::services::services_toml_path()?;
    let registry = ServiceRegistry::load_or_empty(&path)?;

    if !registry.entries.is_empty() {
        println!("Stopping services…");
    }

    let results = registry.run_field(ServiceField::Stop, false, true);

    if !results.is_empty() {
        for r in &results {
            if r.success {
                println!("  ✓ {}", r.service);
            } else {
                eprintln!("  ✗ {}: {}", r.service, r.output);
            }
        }
    }

    crate::cli::status::execute().await
}

// ---------------------------------------------------------------------------
// Platform-specific daemon control
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
fn start_daemon() -> Result<(), GhostError> {
    let uid = get_uid()?;
    let plist = dirs::home_dir()
        .ok_or_else(|| GhostError::Other("cannot determine home directory".into()))?
        .join("Library/LaunchAgents/com.ghost.daemon.plist");

    let status = Command::new("launchctl")
        .args([
            "bootstrap",
            &format!("gui/{uid}"),
            &plist.display().to_string(),
        ])
        .status()
        .map_err(|e| std::io::Error::new(e.kind(), format!("failed to run launchctl: {e}")))?;

    if !status.success() {
        return Err(GhostError::Other("launchctl bootstrap failed".into()));
    }

    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn start_daemon() -> Result<(), GhostError> {
    crate::systemd::start("ghost-daemon").map_err(GhostError::Other)
}

#[cfg(target_os = "macos")]
fn stop_daemon() {
    let uid = match get_uid() {
        Ok(u) => u,
        Err(e) => {
            eprintln!("  Warning: could not determine UID: {e}");
            return;
        }
    };

    let ok = Command::new("launchctl")
        .args(["bootout", &format!("gui/{uid}/com.ghost.daemon")])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success());

    if !ok {
        eprintln!(
            "  Warning: launchctl bootout may have failed (daemon may not have been running)"
        );
    }
}

#[cfg(not(target_os = "macos"))]
fn stop_daemon() {
    let ok = crate::systemd::systemctl_status(&["disable", "--now", "ghost-daemon"])
        .is_ok_and(|s| s.success());

    if !ok {
        eprintln!(
            "  Warning: systemctl disable --now may have failed (daemon may not have been running)"
        );
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
fn get_uid() -> Result<String, GhostError> {
    let output = Command::new("id")
        .arg("-u")
        .output()
        .map_err(|e| std::io::Error::new(e.kind(), format!("failed to run id: {e}")))?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
