use crate::error::GhostError;

#[tracing::instrument(skip_all)]
pub async fn execute() -> Result<(), GhostError> {
    let _observability = crate::observability::init_for_daemon()?;
    crate::daemon::run().await
}
