use clap::Args;

use crate::error::GhostError;

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
    crate::onboarding::wizard::run(args).await
}
