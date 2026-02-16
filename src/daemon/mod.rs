use crate::error::GhostError;

#[tracing::instrument(skip_all)]
pub async fn run() -> Result<(), GhostError> {
    Err(GhostError::NotYetImplemented { command: "daemon" })
}
