use clap::Subcommand;

use crate::error::GhostError;

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    Get { key: String },
    Set { key: String, value: String },
}

#[tracing::instrument(skip_all)]
pub async fn execute(_command: ConfigCommand) -> Result<(), GhostError> {
    Err(GhostError::NotYetImplemented { command: "config" })
}
