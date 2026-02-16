use logfire::config::ConsoleOptions;
use thiserror::Error;

static LIVE_TEST_OBSERVABILITY_INIT: std::sync::Once = std::sync::Once::new();

#[derive(Debug, Error)]
pub enum ObservabilityError {
    #[error("failed to initialize logfire: {source}")]
    LogfireInit {
        #[source]
        source: logfire::ConfigureError,
    },
}

pub struct DaemonObservability {
    _shutdown_guard: Option<logfire::ShutdownGuard>,
}

impl DaemonObservability {
    fn disabled() -> Self {
        Self {
            _shutdown_guard: None,
        }
    }
}

pub fn init_for_daemon() -> Result<DaemonObservability, ObservabilityError> {
    if running_under_cargo_test() {
        return Ok(DaemonObservability::disabled());
    }

    let _ = dotenvy::dotenv();
    set_default_rust_log_filter();
    let logfire = logfire::configure()
        .with_service_name("GHOST")
        .with_install_panic_handler(true)
        .send_to_logfire(logfire::config::SendToLogfire::IfTokenPresent)
        .with_console(Some(console_options()))
        .finish()
        .map_err(|source| ObservabilityError::LogfireInit { source })?;

    Ok(DaemonObservability {
        _shutdown_guard: Some(logfire.shutdown_guard()),
    })
}

pub fn init_for_live_tests() -> Result<DaemonObservability, ObservabilityError> {
    let mut result: Option<ObservabilityError> = None;
    LIVE_TEST_OBSERVABILITY_INIT.call_once(|| {
        if let Err(error) = init_live_tests_inner() {
            result = Some(error);
        }
    });
    if let Some(error) = result {
        return Err(error);
    }

    Ok(DaemonObservability::disabled())
}

fn init_live_tests_inner() -> Result<(), ObservabilityError> {
    let _ = dotenvy::dotenv();
    set_default_rust_log_filter();
    let logfire = logfire::configure()
        .with_service_name("GHOST")
        .with_install_panic_handler(true)
        .send_to_logfire(logfire::config::SendToLogfire::IfTokenPresent)
        .with_console(Some(console_options()))
        .finish()
        .map_err(|source| ObservabilityError::LogfireInit { source })?;

    let _shutdown_guard = logfire.shutdown_guard();
    Ok(())
}

fn console_options() -> ConsoleOptions {
    ConsoleOptions::default()
        .with_colors(logfire::config::ConsoleColors::Auto)
        .with_include_timestamps(true)
        .with_min_log_level(tracing::Level::INFO)
}

fn running_under_cargo_test() -> bool {
    std::env::var_os("RUST_TEST_THREADS").is_some()
}

fn set_default_rust_log_filter() {
    if std::env::var_os("RUST_LOG").is_some() {
        return;
    }

    // SAFETY: daemon startup sets process env before spawning runtime tasks.
    unsafe {
        std::env::set_var("RUST_LOG", "warn,ghost=info");
    }
}
