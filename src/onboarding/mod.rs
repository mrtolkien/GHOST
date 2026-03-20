pub mod agent;
pub mod config_writer;
pub mod detect;
pub mod discord;
pub mod health;
pub mod provider;
pub mod service_files;
pub mod services;
pub mod wizard;

use crate::config::ProviderKind;

/// Tracks cumulative wizard state across phases.
#[derive(Debug, Default)]
pub struct OnboardingState {
    pub provider: Option<ProviderKind>,
    pub api_key: Option<String>,
    pub model: Option<String>,
    pub context_window: Option<u32>,
    pub discord_token: Option<String>,
    pub discord_user_id: Option<String>,
    pub embeddings: Option<ServiceChoice>,
    pub embedding_model: Option<String>,
    pub search: Option<SearchChoice>,
    pub crawl: Option<ServiceChoice>,
    pub docling: Option<ServiceChoice>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceChoice {
    /// Install via nix profile and run as systemd/launchd service.
    NixNative,
    /// Run in the container stack (podman/docker compose).
    Container,
    /// Use an existing remote endpoint.
    Remote(String),
    /// Skip this service entirely.
    Skip,
}

impl ServiceChoice {
    pub fn from_flag(s: &str) -> Result<Self, OnboardingError> {
        match s {
            "local" | "nix" => Ok(Self::NixNative),
            "container" | "docker" | "podman" => Ok(Self::Container),
            "skip" => Ok(Self::Skip),
            s if s.starts_with("remote:") => Ok(Self::Remote(s[7..].to_string())),
            _ => Err(OnboardingError::InvalidInput(format!(
                "invalid service choice: {s}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchChoice {
    SearxngLocal,
    BraveApi(String),
    SearxngRemote(String),
    Skip,
}

impl SearchChoice {
    pub fn from_flag(s: &str) -> Result<Self, OnboardingError> {
        match s {
            "local" | "searxng" => Ok(Self::SearxngLocal),
            "skip" => Ok(Self::Skip),
            s if s.starts_with("brave:") => Ok(Self::BraveApi(s[6..].to_string())),
            s if s.starts_with("remote:") => Ok(Self::SearxngRemote(s[7..].to_string())),
            _ => Err(OnboardingError::InvalidInput(format!(
                "invalid search choice: {s}"
            ))),
        }
    }
}

/// Module-local error type.
#[derive(Debug, thiserror::Error)]
pub enum OnboardingError {
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("provider validation failed: {0}")]
    ProviderValidation(String),
    #[error("discord validation failed: {0}")]
    DiscordValidation(String),
    #[error("nix install failed: {0}")]
    NixInstall(String),
    #[error("service health check failed: {0}")]
    HealthCheck(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Request(#[from] reqwest::Error),
    #[error(transparent)]
    ProviderInit(#[from] crate::providers::types::ProviderInitError),
}
