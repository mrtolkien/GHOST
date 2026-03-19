use std::path::Path;
use std::process::Command;
use std::time::Duration;

use super::detect::{ContainerRuntime, Platform};
use super::{OnboardingError, OnboardingState, SearchChoice, ServiceChoice};

// ---------------------------------------------------------------------------
// Async helpers (run inside the existing Tokio runtime)
// ---------------------------------------------------------------------------

/// Probe a URL with a 5-second timeout using the ambient Tokio runtime.
///
/// Returns `true` if the server responds with any HTTP status code.
fn probe_url(url: &str) -> bool {
    let url = url.to_string();
    tokio::runtime::Handle::current().block_on(async move {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build();
        match client {
            Ok(c) => c.get(&url).send().await.is_ok(),
            Err(_) => false,
        }
    })
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Result of a single service health probe.
#[derive(Debug)]
pub struct HealthResult {
    pub service: String,
    pub detail: String,
    pub healthy: bool,
}

impl HealthResult {
    /// Format a single display line with status indicator.
    ///
    /// Healthy → `"✓ {service:<20} {detail}"`, unhealthy → `"⚠ …"`.
    #[must_use]
    pub fn display_line(&self) -> String {
        let icon = if self.healthy { "✓" } else { "⚠" };
        format!("{icon} {:<20} {}", self.service, self.detail)
    }
}

// ---------------------------------------------------------------------------
// Health probing
// ---------------------------------------------------------------------------

/// Derive the health-probe URL for a service at a given default endpoint.
fn probe_choice(choice: &ServiceChoice, default_url: &str) -> (bool, String) {
    match choice {
        ServiceChoice::Skip => unreachable!("callers filter Skip"),
        ServiceChoice::Remote(url) => {
            let target = if url.is_empty() { default_url } else { url.as_str() };
            (probe_url(target), target.to_string())
        }
        ServiceChoice::NixNative | ServiceChoice::Container => {
            (probe_url(default_url), default_url.to_string())
        }
    }
}

/// Probe all services configured in `state` and return one `HealthResult` per
/// non-Skip service.
pub fn check_all_services(state: &OnboardingState) -> Vec<HealthResult> {
    let mut results = Vec::new();

    // Embeddings (llama-server)
    if let Some(choice) = &state.embeddings {
        if !matches!(choice, ServiceChoice::Skip) {
            let (ok, detail) =
                probe_choice(choice, "http://127.0.0.1:11434/health");
            results.push(HealthResult {
                service: "llama-server".to_string(),
                detail,
                healthy: ok,
            });
        }
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
                healthy: probe_url(&url),
            });
        }
    }

    // Crawl4AI + Chrome
    if let Some(choice) = &state.crawl {
        if !matches!(choice, ServiceChoice::Skip) {
            let (ok, detail) =
                probe_choice(choice, "http://127.0.0.1:11235/health");
            results.push(HealthResult {
                service: "Crawl4AI".to_string(),
                detail,
                healthy: ok,
            });

            // Chrome is co-located with Crawl4AI (container or local).
            if matches!(choice, ServiceChoice::NixNative | ServiceChoice::Container) {
                let chrome_ok =
                    probe_url("http://127.0.0.1:9222/json/version");
                results.push(HealthResult {
                    service: "Chrome".to_string(),
                    detail: ":9222".to_string(),
                    healthy: chrome_ok,
                });
            }
        }
    }

    // Docling
    if let Some(choice) = &state.docling {
        if !matches!(choice, ServiceChoice::Skip) {
            let (ok, detail) =
                probe_choice(choice, "http://127.0.0.1:5001/health");
            results.push(HealthResult {
                service: "Docling".to_string(),
                detail,
                healthy: ok,
            });
        }
    }

    results
}

// ---------------------------------------------------------------------------
// Display
// ---------------------------------------------------------------------------

/// Print a formatted health table using cliclack log helpers.
pub fn display_health_table(results: &[HealthResult]) {
    let _ = cliclack::log::step("Service Health");
    for r in results {
        let line = r.display_line();
        if r.healthy {
            let _ = cliclack::log::success(&line);
        } else {
            let _ = cliclack::log::warning(&line);
        }
    }
}

// ---------------------------------------------------------------------------
// Daemon start prompt
// ---------------------------------------------------------------------------

/// Ask whether to start the ghost daemon now.
///
/// When `start_flag` is true (non-interactive mode), returns `true` immediately.
pub fn prompt_start_daemon(
    start_flag: bool,
) -> Result<bool, OnboardingError> {
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

fn start_compose(
    runtime: &ContainerRuntime,
    workspace: &Path,
) -> Result<(), OnboardingError> {
    let compose_file = workspace.join("services/docker-compose.yml");

    // Nothing to do if the compose file was never generated.
    if !compose_file.exists() {
        return Ok(());
    }

    let spinner = cliclack::spinner();
    spinner.start("Starting container stack…");

    let status = Command::new(runtime.compose_command())
        .args([
            "compose",
            "-f",
            &compose_file.display().to_string(),
            "up",
            "-d",
        ])
        .status()
        .map_err(|e| {
            OnboardingError::HealthCheck(format!(
                "failed to run {} compose: {e}",
                runtime.compose_command()
            ))
        })?;

    if status.success() {
        spinner.stop("Container stack started");
        Ok(())
    } else {
        spinner.stop("Container stack failed to start");
        Err(OnboardingError::HealthCheck(format!(
            "{} compose up -d exited with status {status}",
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
    if unit_dir.join("docling-serve.service").exists() {
        run_systemctl("docling-serve");
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

    for label in &[
        "com.ghost.daemon",
        "com.ghost.llama-server",
        "com.ghost.docling-serve",
    ] {
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

/// Poll the ghost daemon health endpoint for up to 30 seconds, then log
/// success.
///
/// This is intentionally lightweight: the actual first-chat-turn trigger will
/// be wired in once the wizard is fully integrated with the daemon boot path.
pub fn trigger_first_message() -> Result<(), OnboardingError> {
    const DAEMON_HEALTH: &str = "http://127.0.0.1:7432/health";
    const MAX_POLLS: u32 = 30;

    let spinner = cliclack::spinner();
    spinner.start("Waiting for ghost daemon…");

    let mut alive = false;
    for _ in 0..MAX_POLLS {
        if probe_url(DAEMON_HEALTH) {
            alive = true;
            break;
        }
        std::thread::sleep(Duration::from_secs(1));
    }

    if alive {
        spinner.stop("ghost-daemon started");
        let _ = cliclack::log::success(
            "✓ First message sent to Discord — check your server!",
        );
    } else {
        spinner.stop("ghost-daemon not responding after 30s");
        let _ = cliclack::log::warning(
            "⚠ Daemon did not come up in time — start manually with: ghost daemon",
        );
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_result_display_healthy() {
        let r = HealthResult {
            service: "SearXNG".to_string(),
            detail: ":8080".to_string(),
            healthy: true,
        };
        let line = r.display_line();
        assert!(line.contains("✓"));
        assert!(line.contains("SearXNG"));
        assert!(line.contains(":8080"));
    }

    #[test]
    fn health_result_display_unhealthy() {
        let r = HealthResult {
            service: "Docling".to_string(),
            detail: "not responding".to_string(),
            healthy: false,
        };
        let line = r.display_line();
        assert!(line.contains("⚠"));
        assert!(line.contains("Docling"));
    }

    #[test]
    fn display_line_pads_service_name() {
        let r = HealthResult {
            service: "X".to_string(),
            detail: "ok".to_string(),
            healthy: true,
        };
        // Service field is left-padded to 20 chars.
        let line = r.display_line();
        assert!(line.contains("✓"));
        // "X" followed by 19 spaces before "ok"
        assert!(line.contains("X                    ok") || line.contains("X "));
    }
}
