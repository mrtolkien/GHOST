use clap::Subcommand;

use crate::error::GhostError;

#[derive(Debug, Subcommand)]
pub enum KnowledgeCommand {
    Search { query: String },
    Get { id: String },
    Reindex,
}

#[tracing::instrument(skip_all)]
pub async fn execute(_command: KnowledgeCommand) -> Result<(), GhostError> {
    Err(GhostError::NotYetImplemented {
        command: "knowledge",
    })
}
