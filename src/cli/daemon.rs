use crate::error::GhostError;

pub async fn execute() -> Result<(), GhostError> {
    let _observability = crate::observability::init_for_daemon()?;
    crate::daemon::run().await
}
