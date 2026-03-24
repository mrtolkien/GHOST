use std::path::Path;
use std::process::Command;
use std::time::Duration;

use super::detect::{ContainerRuntime, Platform};
use super::{OnboardingError, OnboardingState, SearchChoice, ServiceChoice};
pub use crate::health::{HealthResult, display_health_table, probe_url};

// ---------------------------------------------------------------------------
// Health probing
// ---------------------------------------------------------------------------

/// Derive the health-probe URL for a service at a given default endpoint.
async fn probe_choice(choice: &ServiceChoice, default_url: &str) -> (bool, String) {
    match choice {
        ServiceChoice::Skip | ServiceChoice::Native => {
            unreachable!("callers filter Skip and Native")
        }
        ServiceChoice::Remote(url) => {
            let target = if url.is_empty() {
                default_url
            } else {
                url.as_str()
            };
            (probe_url(target).await, target.to_string())
        }
        ServiceChoice::NixNative | ServiceChoice::Container => {
            (probe_url(default_url).await, default_url.to_string())
        }
    }
}

/// Probe all services configured in `state` and return one `HealthResult` per
/// non-Skip service.
pub async fn check_all_services(state: &OnboardingState) -> Vec<HealthResult> {
    let mut results = Vec::new();

    // Embeddings (llama-server)
    if let Some(choice) = &state.embeddings
        && !matches!(choice, ServiceChoice::Skip)
    {
        let (ok, detail) = probe_choice(choice, "http://127.0.0.1:11434/health").await;
        results.push(HealthResult {
            service: "llama-server".to_string(),
            detail,
            healthy: ok,
        });
    }

    // Search (SearXNG)
    if let Some(choice) = &state.search {
        let (url, label) = match choice {
            SearchChoice::SearxngLocal => {
                ("http://127.0.0.1:8080".to_string(), ":8080".to_string())
            }
            SearchChoice::SearxngRemote(u) => (u.clone(), u.clone()),
            SearchChoice::BraveApi(_) | SearchChoice::Skip => {
                // BraveApi has no local endpoint; Skip is not probed.
                ("".to_string(), "".to_string())
            }
        };

        if !url.is_empty() {
            results.push(HealthResult {
                service: "SearXNG".to_string(),
                detail: label,
                healthy: probe_url(&url).await,
            });
        }
    }

    // Crawl4AI + Chrome
    if let Some(choice) = &state.crawl
        && !matches!(choice, ServiceChoice::Skip)
    {
        let (ok, detail) = probe_choice(choice, "http://127.0.0.1:11235/health").await;
        results.push(HealthResult {
            service: "Crawl4AI".to_string(),
            detail,
            healthy: ok,
        });

        // Chrome is co-located with Crawl4AI (container or local).
        if matches!(choice, ServiceChoice::NixNative | ServiceChoice::Container) {
            let chrome_ok = probe_url("http://127.0.0.1:9222/json/version").await;
            results.push(HealthResult {
                service: "Chrome".to_string(),
                detail: ":9222".to_string(),
                healthy: chrome_ok,
            });
        }
    }

    // Docling — Native runs on-demand (no persistent service to probe).
    if let Some(choice) = &state.docling
        && !matches!(choice, ServiceChoice::Skip | ServiceChoice::Native)
    {
        let (ok, detail) = probe_choice(choice, "http://127.0.0.1:5001/health").await;
        results.push(HealthResult {
            service: "Docling".to_string(),
            detail,
            healthy: ok,
        });
    }

    results
}

// ---------------------------------------------------------------------------
// Daemon start prompt
// ---------------------------------------------------------------------------

/// Ask whether to start the ghost daemon now.
///
/// When `start_flag` is true (non-interactive mode), returns `true` immediately.
pub fn prompt_start_daemon(start_flag: bool) -> Result<bool, OnboardingError> {
    if start_flag {
        return Ok(true);
    }
    let answer = cliclack::confirm("Start the ghost daemon now?")
        .initial_value(true)
        .interact()?;
    Ok(answer)
}

// ---------------------------------------------------------------------------
// Service launcher
// ---------------------------------------------------------------------------

/// Start the container stack and native services for the current platform.
///
/// Uses `std::process::Command` throughout (not async) — the onboarding wizard
/// is sync-at-heart even though it lives inside a Tokio runtime.
pub fn start_all_services(
    platform: &Platform,
    runtime: Option<&ContainerRuntime>,
    workspace: &Path,
) -> Result<(), OnboardingError> {
    // Start container stack if a runtime is available.
    if let Some(rt) = runtime {
        start_compose(rt, workspace)?;
    }

    // Start native services.
    match platform {
        Platform::Linux | Platform::Other(_) => start_systemd_services(),
        Platform::MacOs => start_launchd_services(),
    }

    Ok(())
}

fn start_compose(runtime: &ContainerRuntime, workspace: &Path) -> Result<(), OnboardingError> {
    let compose_file = workspace.join("services/docker-compose.yml");

    // Nothing to do if the compose file was never generated.
    if !compose_file.exists() {
        return Ok(());
    }

    let spinner = cliclack::spinner();
    spinner.start("Starting container stack…");

    let output = Command::new(runtime.compose_command())
        .args([
            "compose",
            "-f",
            &compose_file.display().to_string(),
            "up",
            "-d",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .map_err(|e| {
            OnboardingError::HealthCheck(format!(
                "failed to run {} compose: {e}",
                runtime.compose_command()
            ))
        })?;

    if output.status.success() {
        spinner.stop("Container stack started");
        Ok(())
    } else {
        spinner.stop("Container stack failed to start");
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(OnboardingError::HealthCheck(format!(
            "{} compose up -d failed:\n{stderr}",
            runtime.compose_command()
        )))
    }
}

fn start_systemd_services() {
    let spinner = cliclack::spinner();
    spinner.start("Starting systemd user services…");

    // Ghost daemon — always present after onboarding.
    run_systemctl("ghost-daemon");

    // Optional units — only enable if the unit file was installed.
    let unit_dir = dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/etc/xdg"))
        .join("systemd/user");

    if unit_dir.join("llama-server.service").exists() {
        run_systemctl("llama-server");
    }

    spinner.stop("Systemd services started");
}

/// Enable and start a single systemd user unit; ignores failures (best-effort).
fn run_systemctl(unit: &str) {
    let _ = Command::new("systemctl")
        .args(["--user", "enable", "--now", unit])
        .status();
}

fn start_launchd_services() {
    let spinner = cliclack::spinner();
    spinner.start("Starting launchd services…");

    let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("/tmp"));
    let agents_dir = home.join("Library/LaunchAgents");

    for label in &["com.ghost.daemon", "com.ghost.llama-server"] {
        let plist = agents_dir.join(format!("{label}.plist"));
        if plist.exists() {
            run_launchctl(&plist.display().to_string());
        }
    }

    spinner.stop("LaunchAgents started");
}

/// Bootstrap a single LaunchAgent plist; ignores failures (best-effort).
fn run_launchctl(plist_path: &str) {
    // `id -u` gives us the uid for the `gui/<uid>` session target.
    let uid = Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();
    let uid = uid.trim();

    let _ = Command::new("launchctl")
        .args(["bootstrap", &format!("gui/{uid}"), plist_path])
        .status();
}

// ---------------------------------------------------------------------------
// First message trigger
// ---------------------------------------------------------------------------

/// Poll the service manager until the ghost daemon is active (up to 30s).
///
/// On macOS checks launchctl, on Linux checks systemctl.
pub async fn trigger_first_message() -> Result<(), OnboardingError> {
    const MAX_POLLS: u32 = 30;

    let spinner = cliclack::spinner();
    spinner.start("Waiting for ghost daemon…");

    let mut alive = false;
    for _ in 0..MAX_POLLS {
        if is_daemon_active() {
            alive = true;
            break;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }

    if alive {
        spinner.stop("ghost-daemon started");
        let _ = cliclack::log::success("First message sent to Discord — check your server!");
    } else {
        spinner.stop("ghost-daemon not responding after 30s");
        let _ = cliclack::log::warning(
            "Daemon did not come up in time — start manually with: ghost daemon",
        );
    }

    Ok(())
}

/// Check whether the ghost-daemon service is active via the platform service
/// manager.
fn is_daemon_active() -> bool {
    if cfg!(target_os = "macos") {
        let uid = Command::new("id")
            .arg("-u")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .unwrap_or_default();
        let uid = uid.trim();
        Command::new("launchctl")
            .args(["print", &format!("gui/{uid}/com.ghost.daemon")])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    } else {
        Command::new("systemctl")
            .args(["--user", "is-active", "ghost-daemon"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
}
