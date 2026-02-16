use clap::Subcommand;

use crate::error::GhostError;

#[derive(Debug, Subcommand)]
pub enum AuthCommand {
    Codex,
    Status,
    Revoke,
}

#[tracing::instrument(skip_all)]
pub async fn execute(_command: AuthCommand) -> Result<(), GhostError> {
    Err(GhostError::NotYetImplemented { command: "auth" })
}
