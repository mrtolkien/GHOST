use thiserror::Error;

#[derive(Debug, Error)]
pub enum ObservabilityError {
    #[error("failed to initialize logfire: {source}")]
    LogfireInit {
        #[source]
        source: logfire::ConfigureError,
    },
}

pub struct DaemonObservability {
    _shutdown_guard: logfire::ShutdownGuard,
}

#[tracing::instrument(skip_all)]
pub fn init_for_daemon() -> Result<DaemonObservability, ObservabilityError> {
    let _ = dotenvy::dotenv();
    let logfire = logfire::configure()
        .with_service_name("ghost")
        .with_install_panic_handler(true)
        .send_to_logfire(logfire::config::SendToLogfire::IfTokenPresent)
        .finish()
        .map_err(|source| ObservabilityError::LogfireInit { source })?;

    Ok(DaemonObservability {
        _shutdown_guard: logfire.shutdown_guard(),
    })
}
