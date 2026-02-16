use clap::Subcommand;

use crate::error::GhostError;

#[derive(Debug, Subcommand)]
pub enum JobCommand {
    List,
    Validate { path: String },
    Run { name: String },
    Logs { name: Option<String> },
}

#[tracing::instrument(skip_all)]
pub async fn execute(_command: JobCommand) -> Result<(), GhostError> {
    Err(GhostError::NotYetImplemented { command: "job" })
}
