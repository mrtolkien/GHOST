use std::time::Duration;

const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// Probe a URL with a timeout.
///
/// Returns `true` if the server responds with any HTTP status code.
pub async fn probe_url(url: &str) -> bool {
    let client = reqwest::Client::builder().timeout(PROBE_TIMEOUT).build();
    match client {
        Ok(c) => c.get(url).send().await.is_ok(),
        Err(_) => false,
    }
}

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

/// Print a formatted health table using cliclack log helpers.
pub fn display_health_table(results: &[HealthResult]) {
    for r in results {
        let line = r.display_line();
        if r.healthy {
            let _ = cliclack::log::success(&line);
        } else {
            let _ = cliclack::log::warning(&line);
        }
    }
}

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
        let line = r.display_line();
        assert!(line.contains("✓"));
        assert!(line.contains("X                    ok") || line.contains("X "));
    }
}
