use std::path::{Path, PathBuf};
use std::process::Command;

use clap::Args;

use crate::error::GhostError;
use crate::services::{ServiceField, ServiceRegistry};

/// CLI arguments for `ghost reset`.
#[derive(Debug, Args)]
pub struct ResetArgs {
    /// Skip confirmation prompt
    #[arg(long, short)]
    pub yes: bool,

    /// Also remove config directory (~/.config/ghost)
    #[arg(long)]
    pub include_config: bool,
}

/// Stop all services, remove service files, and delete the workspace.
pub async fn execute(args: ResetArgs) -> Result<(), GhostError> {
    if !args.yes {
        let confirmed =
            cliclack::confirm("This will stop all services and delete your workspace. Continue?")
                .initial_value(false)
                .interact()
                .map_err(|e| std::io::Error::other(e.to_string()))?;

        if !confirmed {
            println!("Aborted.");
            return Ok(());
        }
    }

    // Try to load config for workspace path; fall back to default if config is broken.
    let workspace = crate::config::load()
        .map(|c| c.workspace)
        .unwrap_or_else(|_| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("/tmp"))
                .join("GHOST")
        });

    stop_services(&workspace);
    remove_service_files();
    remove_dir_logged("Workspace", &workspace);
    remove_log_dir();

    if args.include_config
        && let Ok(config_dir) = crate::config::config_dir()
    {
        remove_dir_logged("Config", &config_dir);
    }

    println!("\nReset complete. Run `ghost init` to set up again.");
    Ok(())
}

// ---------------------------------------------------------------------------
// Service shutdown
// ---------------------------------------------------------------------------

fn stop_services(workspace: &Path) {
    println!("Stopping services…");

    // Always stop the daemon via the platform service manager — this is not
    // managed by services.toml because it must be shut down before we can
    // attempt any registry-based teardown.
    if cfg!(target_os = "macos") {
        stop_launchd("com.ghost.daemon");
    } else if crate::systemd::systemctl_status(&["disable", "--now", "ghost-daemon"])
        .is_ok_and(|s| s.success())
    {
        println!("  Stopped ghost-daemon");
    }

    // Try registry-based shutdown first.  If the registry exists and has
    // entries, use it; otherwise fall back to the hardcoded behaviour so
    // that installations created before services.toml was introduced still
    // work correctly.
    let services_toml = workspace.join("services/services.toml");
    match ServiceRegistry::load_or_empty(&services_toml) {
        Ok(reg) if !reg.entries.is_empty() => {
            let results = reg.run_field(ServiceField::Stop, false, true);
            for r in results {
                if r.success {
                    println!("  Stopped {}", r.service);
                } else {
                    eprintln!("  Warning: stopping {} failed: {}", r.service, r.output);
                }
            }
        }
        Ok(_) => {
            // Registry missing or empty — use hardcoded fallback.
            stop_services_legacy(workspace);
        }
        Err(e) => {
            eprintln!(
                "  Warning: could not read services.toml ({e}); falling back to legacy shutdown"
            );
            stop_services_legacy(workspace);
        }
    }
}

/// Legacy (pre-services.toml) shutdown: stops known native services and the
/// container stack.  Kept as a fallback for older installations.
fn stop_services_legacy(workspace: &Path) {
    if cfg!(target_os = "macos") {
        stop_launchd("com.ghost.llama-server");
        stop_launchd("com.ghost.docling-serve");
    } else {
        if crate::systemd::systemctl_status(&["disable", "--now", "llama-server"])
            .is_ok_and(|s| s.success())
        {
            println!("  Stopped llama-server");
        }
        if crate::systemd::systemctl_status(&["disable", "--now", "docling-serve"])
            .is_ok_and(|s| s.success())
        {
            println!("  Stopped docling-serve");
        }
    }

    // Stop container stack
    let compose_file = workspace.join("services/docker-compose.yml");
    if compose_file.exists() {
        for runtime in ["podman", "docker"] {
            let result = Command::new(runtime)
                .args(["compose", "-f", &compose_file.display().to_string(), "down"])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
            if result.is_ok_and(|s| s.success()) {
                println!("  Container stack stopped ({runtime})");
                break;
            }
        }
    }
}

fn stop_launchd(label: &str) {
    let uid = Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();
    let uid = uid.trim();

    let ok = Command::new("launchctl")
        .args(["bootout", &format!("gui/{uid}/{label}")])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success());

    if ok {
        println!("  Stopped {label}");
    }
}

// ---------------------------------------------------------------------------
// Service file removal
// ---------------------------------------------------------------------------

fn remove_service_files() {
    println!("Removing service files…");

    if cfg!(target_os = "macos") {
        let agents_dir = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("Library/LaunchAgents");

        for label in [
            "com.ghost.daemon",
            "com.ghost.llama-server",
            "com.ghost.docling-serve",
        ] {
            remove_file_logged(&agents_dir.join(format!("{label}.plist")));
        }
    } else {
        let unit_dir =
            crate::systemd::unit_dir().unwrap_or_else(|_| PathBuf::from("/etc/xdg/systemd/user"));

        for unit in [
            "ghost-daemon.service",
            "llama-server.service",
            "docling-serve.service",
        ] {
            remove_file_logged(&unit_dir.join(unit));
        }

        // Reload unit files so systemd forgets removed units.
        crate::systemd::daemon_reload();
    }
}

// ---------------------------------------------------------------------------
// Log directory removal
// ---------------------------------------------------------------------------

fn remove_log_dir() {
    if let Some(data_dir) = dirs::data_dir() {
        let log_dir = data_dir.join("ghost/logs");
        remove_dir_logged("Logs", &log_dir);
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn remove_file_logged(path: &Path) {
    if path.exists() {
        match std::fs::remove_file(path) {
            Ok(()) => println!("  Removed {}", path.display()),
            Err(e) => eprintln!("  Warning: could not remove {}: {e}", path.display()),
        }
    }
}

fn remove_dir_logged(label: &str, path: &Path) {
    if path.exists() {
        match std::fs::remove_dir_all(path) {
            Ok(()) => println!("{label} removed ({})", path.display()),
            Err(e) => eprintln!("Warning: could not remove {}: {e}", path.display()),
        }
    }
}
