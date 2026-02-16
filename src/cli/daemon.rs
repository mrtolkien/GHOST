use crate::error::GhostError;

#[tracing::instrument(skip_all)]
pub async fn execute() -> Result<(), GhostError> {
    crate::daemon::run().await
}
