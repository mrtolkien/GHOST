use std::sync::OnceLock;

use opentelemetry::KeyValue;
use opentelemetry::trace::TracerProvider;
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::trace::SdkTracerProvider;
use thiserror::Error;
use tracing_subscriber::Layer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// Holds the tracer provider for the test process so the export pipeline
/// stays alive until process exit. Initialized exactly once.
static TEST_PROVIDER: OnceLock<SdkTracerProvider> = OnceLock::new();

#[derive(Debug, Error)]
pub enum ObservabilityError {
    #[error("failed to initialize OpenTelemetry exporter: {source}")]
    OtelInit {
        #[source]
        source: opentelemetry_otlp::ExporterBuildError,
    },
}

pub struct DaemonObservability {
    _provider: Option<SdkTracerProvider>,
}

impl DaemonObservability {
    fn disabled() -> Self {
        Self { _provider: None }
    }
}

impl Drop for DaemonObservability {
    fn drop(&mut self) {
        if let Some(provider) = self._provider.take()
            && let Err(e) = provider.shutdown()
        {
            eprintln!("OpenTelemetry shutdown error: {e}");
        }
    }
}

pub fn init() -> Result<DaemonObservability, ObservabilityError> {
    if running_under_cargo_test() {
        return Ok(DaemonObservability::disabled());
    }

    crate::config::load_dotenv_from_config_dir();
    set_default_rust_log_filter();
    install_panic_handler();

    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_ansi(true)
        .with_timer(tracing_subscriber::fmt::time::SystemTime);

    let env_filter = tracing_subscriber::EnvFilter::from_default_env();

    if std::env::var_os("OTEL_EXPORTER_OTLP_ENDPOINT").is_some() {
        let provider = build_tracer_provider("production")?;
        let tracer = provider.tracer("ghost");
        let otel_layer = tracing_opentelemetry::layer()
            .with_tracer(tracer)
            .with_filter(tracing_subscriber::filter::LevelFilter::INFO);

        tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt_layer)
            .with(otel_layer)
            .init();

        Ok(DaemonObservability {
            _provider: Some(provider),
        })
    } else {
        tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt_layer)
            .init();

        Ok(DaemonObservability::disabled())
    }
}

pub fn init_for_live_tests() -> Result<DaemonObservability, ObservabilityError> {
    static INIT: std::sync::Once = std::sync::Once::new();

    let mut result: Option<ObservabilityError> = None;
    INIT.call_once(|| {
        crate::config::load_dotenv_from_config_dir();
        set_default_rust_log_filter_for_tests();
        install_panic_handler();

        let fmt_layer = tracing_subscriber::fmt::layer()
            .with_ansi(true)
            .with_timer(tracing_subscriber::fmt::time::SystemTime);

        let env_filter = tracing_subscriber::EnvFilter::from_default_env();

        if std::env::var_os("OTEL_EXPORTER_OTLP_ENDPOINT").is_some() {
            match build_tracer_provider("test") {
                Ok(provider) => {
                    let tracer = provider.tracer("ghost");
                    let otel_layer = tracing_opentelemetry::layer()
                        .with_tracer(tracer)
                        .with_filter(tracing_subscriber::filter::LevelFilter::INFO);

                    tracing_subscriber::registry()
                        .with(env_filter)
                        .with(fmt_layer)
                        .with(otel_layer)
                        .init();

                    let _ = TEST_PROVIDER.set(provider);
                }
                Err(e) => {
                    result = Some(e);
                }
            }
        } else {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(fmt_layer)
                .init();
        }
    });
    if let Some(error) = result {
        return Err(error);
    }

    Ok(DaemonObservability::disabled())
}

fn build_tracer_provider(environment: &str) -> Result<SdkTracerProvider, ObservabilityError> {
    let service_name = std::env::var("OTEL_SERVICE_NAME").unwrap_or_else(|_| "GHOST".to_owned());

    let hostname = gethostname::gethostname();
    let hostname = hostname.to_string_lossy().into_owned();

    let resource = Resource::builder()
        .with_attributes([
            KeyValue::new("service.name", service_name),
            KeyValue::new("deployment.environment.name", environment.to_owned()),
            KeyValue::new("host.name", hostname),
        ])
        .build();

    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_http()
        .build()
        .map_err(|source| ObservabilityError::OtelInit { source })?;

    let provider = SdkTracerProvider::builder()
        .with_resource(resource)
        .with_batch_exporter(exporter)
        .build();

    Ok(provider)
}

fn install_panic_handler() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = info.payload_as_str().unwrap_or("unknown panic");
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "unknown".to_owned());
        tracing::error!(
            panic.message = message,
            panic.location = location,
            "panic occurred"
        );
        prev(info);
    }));
}

fn running_under_cargo_test() -> bool {
    std::env::var_os("RUST_TEST_THREADS").is_some()
}

fn set_default_rust_log_filter() {
    if std::env::var_os("RUST_LOG").is_some() {
        return;
    }

    // RUST_LOG controls both console output and what gets exported via OTLP.
    //
    // To see provider request/response bodies, set:
    //   RUST_LOG=warn,ghost=info,ghost::providers=debug
    //
    // SAFETY: daemon startup sets process env before spawning runtime tasks.
    // TODO: remove chromiumoxide=error filter once upstream fixes
    // camelCase CDP event deserialization (chromiumoxide#266). Chrome
    // 140+ sends events that chromiumoxide can't parse, spamming
    // "WS Invalid message: data did not match any variant of untagged
    // enum Message" warnings hundreds of times per minute.
    unsafe {
        std::env::set_var(
            "RUST_LOG",
            "warn,ghost=info,chromiumoxide=error,\
             usvg=off,resvg=off,fontdb=off,html5ever=off",
        );
    }
}

fn set_default_rust_log_filter_for_tests() {
    if std::env::var_os("RUST_LOG").is_some() {
        return;
    }

    // Quieter defaults for tests: suppress provider request/response body
    // logging (INFO-level in ghost::providers) which produces megabytes of
    // output. Raw requests are still saved to debug/requests/ on disk via
    // the debug.save_requests config flag.
    //
    // Override with RUST_LOG=warn,ghost=info,ghost::providers=debug to
    // re-enable verbose provider logging when needed.
    //
    // SAFETY: test init sets process env before spawning runtime tasks.
    // TODO: remove chromiumoxide=error once chromiumoxide#266 is fixed.
    unsafe {
        std::env::set_var(
            "RUST_LOG",
            "warn,ghost=info,ghost::providers=warn,chromiumoxide=error,\
             usvg=off,resvg=off,fontdb=off,html5ever=off",
        );
    }
}
