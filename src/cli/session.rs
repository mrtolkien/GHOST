use clap::Subcommand;

use crate::error::GhostError;

#[derive(Debug, Subcommand)]
pub enum SessionCommand {
    List,
    Show { id: String },
}

#[tracing::instrument(skip_all)]
pub async fn execute(_command: SessionCommand) -> Result<(), GhostError> {
    Err(GhostError::NotYetImplemented { command: "session" })
}
