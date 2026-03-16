use std::time::Duration;

use serde::Deserialize;
use tracing::debug;

const CDP_PORTS: std::ops::RangeInclusive<u16> = 9222..=9229;
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);

/// A discovered CDP endpoint.
#[derive(Debug, Clone)]
pub struct DiscoveredBrowser {
    pub host: String,
    pub port: u16,
    pub cdp_url: String,
    pub browser_version: Option<String>,
}

#[derive(Deserialize)]
struct CdpVersionResponse {
    #[serde(rename = "Browser")]
    browser: Option<String>,
    #[serde(rename = "webSocketDebuggerUrl")]
    web_socket_debugger_url: Option<String>,
}

/// Discover CDP endpoints on localhost and Tailscale peers.
///
/// Probes all ports in `CDP_PORTS` on localhost and any online Tailscale
/// peers concurrently. Returns all endpoints that respond to `/json/version`.
pub async fn discover() -> Vec<DiscoveredBrowser> {
    let client = match reqwest::Client::builder().timeout(PROBE_TIMEOUT).build() {
        Ok(c) => c,
        Err(e) => {
            debug!("failed to build discovery client: {e}");
            return vec![];
        }
    };

    let mut targets = Vec::new();

    // Localhost
    for port in CDP_PORTS {
        targets.push(("127.0.0.1".to_string(), port));
    }

    // Tailscale peers
    if let Ok(peers) = tailscale_peers().await {
        for ip in peers {
            for port in CDP_PORTS {
                targets.push((ip.clone(), port));
            }
        }
    }

    // Probe all targets concurrently
    let mut tasks = Vec::new();
    for (host, port) in targets {
        let client = client.clone();
        tasks.push(tokio::spawn(async move {
            probe_cdp(&client, &host, port).await
        }));
    }

    let mut found = Vec::new();
    for task in tasks {
        if let Ok(Ok(Some(browser))) = task.await {
            found.push(browser);
        }
    }

    debug!(count = found.len(), "CDP discovery complete");
    found
}

async fn probe_cdp(
    client: &reqwest::Client,
    host: &str,
    port: u16,
) -> Result<Option<DiscoveredBrowser>, reqwest::Error> {
    let url = format!("http://{host}:{port}/json/version");
    let resp = client.get(&url).send().await?;
    let version: CdpVersionResponse = resp.json().await?;

    let cdp_url = version
        .web_socket_debugger_url
        .unwrap_or_else(|| format!("ws://{host}:{port}"));

    Ok(Some(DiscoveredBrowser {
        host: host.to_string(),
        port,
        cdp_url,
        browser_version: version.browser,
    }))
}

/// Get Tailscale peer IPs via `tailscale status --json`.
///
/// Returns only online peers' IPv4 addresses. Fails gracefully if
/// Tailscale is not installed or not running.
async fn tailscale_peers() -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
    let output = tokio::process::Command::new("tailscale")
        .args(["status", "--json"])
        .output()
        .await?;

    if !output.status.success() {
        return Err("tailscale status failed".into());
    }

    #[derive(Deserialize)]
    struct TailscaleStatus {
        #[serde(rename = "Peer")]
        peer: Option<std::collections::HashMap<String, TailscalePeer>>,
    }

    #[derive(Deserialize)]
    struct TailscalePeer {
        #[serde(rename = "TailscaleIPs")]
        tailscale_ips: Option<Vec<String>>,
        #[serde(rename = "Online")]
        online: Option<bool>,
    }

    let status: TailscaleStatus = serde_json::from_slice(&output.stdout)?;
    let mut ips = Vec::new();
    if let Some(peers) = status.peer {
        for peer in peers.values() {
            if peer.online.unwrap_or(false) {
                let Some(ref peer_ips) = peer.tailscale_ips else {
                    continue;
                };
                // Take IPv4 addresses only
                for ip in peer_ips {
                    if !ip.contains(':') {
                        ips.push(ip.clone());
                    }
                }
            }
        }
    }
    Ok(ips)
}
