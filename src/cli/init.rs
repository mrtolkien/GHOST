use crate::error::GhostError;

#[tracing::instrument(skip_all)]
pub async fn execute() -> Result<(), GhostError> {
    let config = crate::config::load()?;
    crate::config_workspace::bootstrap_workspace(&config)?;
    Ok(())
}
