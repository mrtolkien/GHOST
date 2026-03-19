use clap::Args;

use crate::error::GhostError;
use crate::onboarding::detect;

/// Arguments for the `ghost init` onboarding wizard.
#[derive(Args, Debug)]
pub struct InitArgs {
    /// LLM provider: openrouter, anthropic, kimi, openai-oauth
    #[arg(long)]
    pub provider: Option<String>,

    /// API key for the selected provider
    #[arg(long)]
    pub api_key: Option<String>,

    /// Model ID (e.g. "anthropic/claude-sonnet-4")
    #[arg(long)]
    pub model: Option<String>,

    /// Context window size in tokens
    #[arg(long)]
    pub context_window: Option<u32>,

    /// Discord bot token
    #[arg(long)]
    pub discord_token: Option<String>,

    /// Discord user ID (numeric)
    #[arg(long)]
    pub discord_user: Option<String>,

    /// Embeddings setup: local, remote:<url>, skip
    #[arg(long)]
    pub embeddings: Option<String>,

    /// Web search setup: local, brave:<key>, remote:<url>, skip
    #[arg(long)]
    pub search: Option<String>,

    /// Web fetch setup: local, remote:<url>, skip
    #[arg(long)]
    pub crawl: Option<String>,

    /// Document processing: local, container, remote:<url>, skip
    #[arg(long)]
    pub docling: Option<String>,

    /// Start all services after setup
    #[arg(long)]
    pub start: bool,
}

#[tracing::instrument(skip_all)]
pub async fn execute(args: InitArgs) -> Result<(), GhostError> {
    let _ = args;

    // Phase 0: Detection
    let env = detect::detect().await;

    if !env.nix_installed {
        eprintln!("Nix is required but not installed.");
        eprintln!("Install it from: https://install.determinate.systems/nix");
        return Err(GhostError::Other("Nix is not installed".into()));
    }

    // Display detection results
    cliclack::intro("GHOST — First-time setup")
        .map_err(|e| GhostError::Other(e.to_string()))?;
    display_detection_results(&env);

    // TODO: Phases 1-5 will be added in subsequent tasks
    cliclack::outro("Detection phase complete. More phases coming soon.")
        .map_err(|e| GhostError::Other(e.to_string()))?;

    Ok(())
}

fn display_detection_results(env: &detect::DetectedEnvironment) {
    let _ = cliclack::log::success("Nix installed");
    let _ = cliclack::log::success(format!("Platform: {:?}", env.platform));

    match &env.container_runtime {
        Some(detect::ContainerRuntime::Podman) => {
            let _ = cliclack::log::success("Container runtime: Podman");
        }
        Some(detect::ContainerRuntime::Docker) => {
            let _ = cliclack::log::success("Container runtime: Docker");
        }
        None => {
            let _ = cliclack::log::warning("No container runtime found (podman or docker)");
        }
    }

    if env.llama_server_in_path {
        let _ = cliclack::log::success("llama-server found in PATH");
    } else {
        let _ = cliclack::log::info("llama-server not found in PATH");
    }

    if env.docling_serve_in_path {
        let _ = cliclack::log::success("docling-serve found in PATH");
    } else {
        let _ = cliclack::log::info("docling-serve not found in PATH");
    }

    if env.existing_config.is_some() {
        let _ = cliclack::log::info("Existing config.toml detected");
    }
}

