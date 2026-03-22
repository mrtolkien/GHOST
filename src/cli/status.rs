use crate::config::{Config, SearchProviderConfig};
use crate::error::GhostError;
use crate::health::{HealthResult, display_health_table, probe_url};

/// Print system status: config validity, daemon status, and service health.
pub async fn execute() -> Result<(), GhostError> {
    let config = check_config();
    check_daemon().await;
    if let Some(cfg) = config {
        check_services(&cfg).await;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

/// Try to load the config, print the result, and return it on success.
fn check_config() -> Option<Config> {
    let config_path = crate::config::config_dir()
        .map(|d| d.join("config.toml"))
        .ok();

    let path_display = config_path
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "(unknown)".to_string());

    let _ = cliclack::log::step(format!("Config  {path_display}"));

    match crate::config::load() {
        Ok(cfg) => {
            let _ = cliclack::log::success("✓ Config valid");
            Some(cfg)
        }
        Err(e) => {
            let _ = cliclack::log::error(format!("✗ {e}"));
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Daemon
// ---------------------------------------------------------------------------

async fn check_daemon() {
    let _ = cliclack::log::step("Daemon");

    let active = is_service_active();
    let result = HealthResult {
        service: "ghost-daemon".to_string(),
        detail: if active {
            "running".to_string()
        } else {
            "not running".to_string()
        },
        healthy: active,
    };

    display_health_table(&[result]);
}

/// Check whether the ghost-daemon unit is active via the platform service
/// manager. Returns `false` on any error (missing binary, no unit, etc.).
fn is_service_active() -> bool {
    if cfg!(target_os = "macos") {
        is_launchd_active()
    } else {
        is_systemd_active()
    }
}

fn is_systemd_active() -> bool {
    std::process::Command::new("systemctl")
        .args(["--user", "is-active", "ghost-daemon"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn is_launchd_active() -> bool {
    let uid = std::process::Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();
    let uid = uid.trim();

    std::process::Command::new("launchctl")
        .args(["print", &format!("gui/{uid}/com.ghost.daemon")])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Services
// ---------------------------------------------------------------------------

async fn check_services(config: &Config) {
    let _ = cliclack::log::step("Services");

    let mut results = Vec::new();

    // Embeddings (llama-server)
    let emb_url = format!("{}/health", config.embeddings.url.trim_end_matches('/'));
    results.push(HealthResult {
        service: "Embeddings".to_string(),
        detail: config.embeddings.url.clone(),
        healthy: probe_url(&emb_url).await,
    });

    // Search
    match &config.web.search_provider {
        SearchProviderConfig::Searxng { url } => {
            results.push(HealthResult {
                service: "Search".to_string(),
                detail: url.clone(),
                healthy: probe_url(url).await,
            });
        }
        SearchProviderConfig::Brave => {
            results.push(HealthResult {
                service: "Search".to_string(),
                detail: "Brave API (key configured)".to_string(),
                healthy: true,
            });
        }
    }

    // Crawl4AI
    if let Some(url) = &config.web.crawl4ai_url {
        let health_url = format!("{}/health", url.trim_end_matches('/'));
        results.push(HealthResult {
            service: "Crawl4AI".to_string(),
            detail: url.clone(),
            healthy: probe_url(&health_url).await,
        });
    }

    // Docling
    if let Some(url) = &config.docling.url {
        let health_url = format!("{}/health", url.trim_end_matches('/'));
        results.push(HealthResult {
            service: "Docling".to_string(),
            detail: url.clone(),
            healthy: probe_url(&health_url).await,
        });
    }

    // Browsers — CDP URLs use ws:// but we probe via HTTP /json/version.
    for browser in &config.web.browsers {
        let probe = cdp_health_url(&browser.cdp_url);
        results.push(HealthResult {
            service: format!("Browser ({})", browser.name),
            detail: browser.cdp_url.clone(),
            healthy: probe_url(&probe).await,
        });
    }

    display_health_table(&results);
}

/// Convert a CDP WebSocket URL to an HTTP health-probe URL.
///
/// `ws://host:port` or `ws://host:port/path` → `http://host:port/json/version`
/// Already-HTTP URLs get `/json/version` appended.
fn cdp_health_url(cdp_url: &str) -> String {
    let http = cdp_url
        .replace("wss://", "https://")
        .replace("ws://", "http://");
    let base = http.split('/').take(3).collect::<Vec<_>>().join("/");
    format!("{base}/json/version")
}
