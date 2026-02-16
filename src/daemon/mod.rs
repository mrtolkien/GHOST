//! WARNING: This file currently contains implementation code. Move logic into
//! dedicated module files before real daemon feature development continues.

use crate::error::GhostError;

#[tracing::instrument(skip_all)]
pub async fn run() -> Result<(), GhostError> {
    let config = crate::config::load()?;
    crate::config::bootstrap_workspace(&config)?;
    let _db = crate::db::connect(&config.workspace).await?;

    Err(GhostError::NotYetImplemented { command: "daemon" })
}
