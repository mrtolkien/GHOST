use crate::error::GhostError;

pub async fn execute() -> Result<(), GhostError> {
    let _observability = crate::observability::init()?;
    crate::daemon::run().await
}
