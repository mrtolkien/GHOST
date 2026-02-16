use crate::error::GhostError;

#[tracing::instrument(skip_all)]
pub async fn execute() -> Result<(), GhostError> {
    Err(GhostError::NotYetImplemented { command: "init" })
}
