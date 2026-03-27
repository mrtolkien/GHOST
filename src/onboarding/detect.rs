use std::path::PathBuf;
use std::time::Duration;

use sysinfo::System;

/// 4 GiB in bytes — minimum RAM for local models.
const LOW_MEMORY_THRESHOLD: u64 = 4 * 1024 * 1024 * 1024;

/// Timeout for HTTP service probes during detection.
const SERVICE_PROBE_TIMEOUT: Duration = Duration::from_secs(2);

/// Platform the binary is running on.
#[derive(Debug, Clone)]
pub enum Platform {
    Linux,
    MacOs,
    Other(String),
}

impl Platform {
    /// Detect at compile time via `cfg!` macros.
    pub fn detect() -> Self {
        if cfg!(target_os = "linux") {
            Self::Linux
        } else if cfg!(target_os = "macos") {
            Self::MacOs
        } else {
            Self::Other(std::env::consts::OS.to_string())
        }
    }

    pub fn is_linux(&self) -> bool {
        matches!(self, Self::Linux)
    }

    pub fn is_macos(&self) -> bool {
        matches!(self, Self::MacOs)
    }
}

/// Container runtime available on the host.
#[derive(Debug, Clone)]
pub enum ContainerRuntime {
    Podman,
    Docker,
}

impl ContainerRuntime {
    /// Build from pre-computed `which` results. Podman is preferred over Docker.
    pub fn from_which_results(podman: bool, docker: bool) -> Option<Self> {
        if podman {
            Some(Self::Podman)
        } else if docker {
            Some(Self::Docker)
        } else {
            None
        }
    }

    /// The CLI command to use for compose operations.
    pub fn compose_command(&self) -> &'static str {
        match self {
            Self::Podman => "podman",
            Self::Docker => "docker",
        }
    }
}

/// Which optional background services are already reachable on localhost.
#[derive(Debug, Default)]
pub struct RunningServices {
    pub llama_server: bool,
    pub searxng: bool,
    pub chrome: bool,
    pub crawl4ai: bool,
    pub docling: bool,
}

/// Snapshot of the host environment gathered during onboarding.
#[derive(Debug)]
pub struct DetectedEnvironment {
    pub nix_installed: bool,
    pub platform: Platform,
    pub container_runtime: Option<ContainerRuntime>,
    pub llama_server_in_path: bool,
    pub services_running: RunningServices,
    pub existing_config: Option<PathBuf>,
    pub existing_env: Option<PathBuf>,
    pub low_memory: bool,
    pub total_memory_bytes: u64,
}

/// Returns `true` when total RAM is below 4 GiB — too little for local models.
pub fn is_low_memory(total_bytes: u64) -> bool {
    total_bytes < LOW_MEMORY_THRESHOLD
}

/// Search PATH for a binary without shelling out.
fn which_exists(binary: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths).any(|dir| {
                let candidate = dir.join(binary);
                candidate.is_file()
            })
        })
        .unwrap_or(false)
}

/// Probe a URL with a 2-second timeout; returns `true` if the server responds.
async fn probe_http(url: &str) -> bool {
    let client = reqwest::Client::builder()
        .timeout(SERVICE_PROBE_TIMEOUT)
        .build();

    match client {
        Ok(c) => c.get(url).send().await.is_ok(),
        Err(_) => false,
    }
}

/// Gather all environment information needed by the onboarding wizard.
pub async fn detect() -> DetectedEnvironment {
    let platform = Platform::detect();

    let nix_installed = which_exists("nix");
    let podman_in_path = which_exists("podman");
    let docker_in_path = which_exists("docker");
    let llama_server_in_path = which_exists("llama-server");

    let container_runtime = ContainerRuntime::from_which_results(podman_in_path, docker_in_path);

    // Probe all services concurrently.
    let (llama_ok, searxng_ok, chrome_ok, crawl4ai_ok, docling_ok) = tokio::join!(
        probe_http("http://127.0.0.1:11434/health"),
        probe_http("http://127.0.0.1:8080"),
        probe_http("http://127.0.0.1:9222/json/version"),
        probe_http("http://127.0.0.1:11235/health"),
        probe_http("http://127.0.0.1:5001/health"),
    );

    let services_running = RunningServices {
        llama_server: llama_ok,
        searxng: searxng_ok,
        chrome: chrome_ok,
        crawl4ai: crawl4ai_ok,
        docling: docling_ok,
    };

    // Check for existing config and .env files — ignore errors (absence is fine).
    let (existing_config, existing_env) = match crate::config::config_dir() {
        Ok(dir) => {
            let config_path = dir.join("config.toml");
            let env_path = dir.join(".env");
            (
                config_path.exists().then_some(config_path),
                env_path.exists().then_some(env_path),
            )
        }
        Err(_) => (None, None),
    };

    // Collect memory info via sysinfo.
    let mut sys = System::new();
    sys.refresh_memory();
    let total_memory_bytes = sys.total_memory();

    DetectedEnvironment {
        nix_installed,
        platform,
        container_runtime,
        llama_server_in_path,
        services_running,
        existing_config,
        existing_env,
        low_memory: is_low_memory(total_memory_bytes),
        total_memory_bytes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_platform() {
        let platform = Platform::detect();
        assert!(matches!(
            platform,
            Platform::Linux | Platform::MacOs | Platform::Other(_)
        ));
    }

    #[test]
    fn container_runtime_prefers_podman() {
        let rt = ContainerRuntime::from_which_results(true, true);
        assert!(matches!(rt, Some(ContainerRuntime::Podman)));
    }

    #[test]
    fn container_runtime_falls_back_to_docker() {
        let rt = ContainerRuntime::from_which_results(false, true);
        assert!(matches!(rt, Some(ContainerRuntime::Docker)));
    }

    #[test]
    fn container_runtime_none_if_neither() {
        let rt = ContainerRuntime::from_which_results(false, false);
        assert!(rt.is_none());
    }

    #[test]
    fn low_memory_detection() {
        assert!(is_low_memory(2 * 1024 * 1024 * 1024));
        assert!(!is_low_memory(8 * 1024 * 1024 * 1024));
    }
}
