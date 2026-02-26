use std::sync::OnceLock;

use logfire::config::ConsoleOptions;
use thiserror::Error;

/// Holds the shutdown guard for the test process so the export pipeline
/// stays alive until process exit. Initialized exactly once.
static TEST_SHUTDOWN_GUARD: OnceLock<logfire::ShutdownGuard> = OnceLock::new();

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
    set_default_logfire_environment();
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
    static INIT: std::sync::Once = std::sync::Once::new();

    let mut result: Option<ObservabilityError> = None;
    INIT.call_once(|| {
        let _ = dotenvy::dotenv();
        set_default_rust_log_filter();
        match logfire::configure()
            .with_service_name("GHOST")
            .with_environment("test")
            .with_install_panic_handler(true)
            .send_to_logfire(logfire::config::SendToLogfire::IfTokenPresent)
            .with_console(Some(console_options()))
            .finish()
        {
            Ok(logfire) => {
                let _ = TEST_SHUTDOWN_GUARD.set(logfire.shutdown_guard());
            }
            Err(source) => {
                result = Some(ObservabilityError::LogfireInit { source });
            }
        }
    });
    if let Some(error) = result {
        return Err(error);
    }

    Ok(DaemonObservability::disabled())
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

fn set_default_logfire_environment() {
    if std::env::var_os("LOGFIRE_ENVIRONMENT").is_some() {
        return;
    }
    // SAFETY: daemon startup sets process env before spawning runtime tasks.
    unsafe {
        std::env::set_var("LOGFIRE_ENVIRONMENT", "production");
    }
}

fn set_default_rust_log_filter() {
    if std::env::var_os("RUST_LOG").is_some() {
        return;
    }

    // RUST_LOG controls both console output and what gets sent to logfire.
    // logfire's ConsoleOptions::with_min_log_level has a bug (0.9.0) where
    // log records bypass the min-level check, so RUST_LOG is the only
    // reliable way to filter console output.
    //
    // To see provider request/response bodies, set:
    //   RUST_LOG=warn,ghost=info,ghost::providers=debug
    //
    // SAFETY: daemon startup sets process env before spawning runtime tasks.
    unsafe {
        std::env::set_var(
            "RUST_LOG",
            "warn,ghost=info,usvg=off,resvg=off,html5ever=off",
        );
    }
}
