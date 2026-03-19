# Self-Hosted OpenTelemetry with SigNoz — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task.
> Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the `logfire` crate with standard OpenTelemetry OTLP export and add
SigNoz to the Docker Compose stack as a self-hosted observability backend.

**Architecture:** Drop `logfire` (Pydantic's proprietary OTel wrapper) in favor of the
standard `opentelemetry-otlp` + `tracing-opentelemetry` crates. The pipeline is
two-layer: console output (always on, via `tracing_subscriber::fmt`) and OTLP export
(conditional, via `OTEL_EXPORTER_OTLP_ENDPOINT`). SigNoz runs in a separate Docker
Compose file as the self-hosted OTel backend.

**Tech Stack:** `opentelemetry 0.31`, `opentelemetry_sdk 0.31`, `opentelemetry-otlp 0.31`
(HTTP/protobuf + reqwest), `tracing-opentelemetry 0.32`, SigNoz v0.116+

**Spec:** `backlog/tasks/4-easy-install/3-opentelemetry.md`

---

## File Map

| Action | File | Purpose |
|--------|------|---------|
| Create | `docker-compose.signoz.yml` | SigNoz services (6 containers) |
| Create | `deploy/common/signoz/otel-collector-config.yaml` | OTel collector pipeline config |
| Modify | `Cargo.toml` | Swap logfire → opentelemetry stack |
| Rewrite | `src/observability.rs` | New OTel pipeline setup |
| Modify | 33 src/ files | `logfire::` → `tracing::` macros |
| Modify | `CLAUDE.md` + `AGENTS.md` | Update dependencies list (identical files) |
| Rewrite | `.agents/skills/tracing/SKILL.md` | Remove logfire references |
| Delete | `.agents/skills/logfire/SKILL.md` | No longer applicable |
| Modify | `.claude/settings.local.json` | Remove mcp\_\_logfire\_\_ entries |
| Modify | `Cargo.toml` [features] | Add `live-tests-observability` |
| Create | `tests/observability_live.rs` | Live test: OTLP export to SigNoz |

---

## Task 1: SigNoz Docker Compose Stack

**Files:**
- Create: `docker-compose.signoz.yml`
- Create: `deploy/common/signoz/otel-collector-config.yaml`

- [ ] **Step 1: Create the OTel collector config**

This config tells the SigNoz collector how to receive OTLP and route it to ClickHouse.
Adapted from SigNoz's official `deploy/docker/otel-collector-config.yaml`.

Create `deploy/common/signoz/otel-collector-config.yaml`:

```yaml
receivers:
  otlp:
    protocols:
      grpc:
        endpoint: 0.0.0.0:4317
      http:
        endpoint: 0.0.0.0:4318

processors:
  batch:
    send_batch_size: 10000
    send_batch_max_size: 11000
    timeout: 10s

exporters:
  clickhousetraces:
    datasource: tcp://clickhouse:9000/signoz_traces
  clickhouselogsexporter:
    datasource: tcp://clickhouse:9000/signoz_logs

extensions:
  health_check:
    endpoint: 0.0.0.0:13133

service:
  extensions: [health_check]
  pipelines:
    traces:
      receivers: [otlp]
      processors: [batch]
      exporters: [clickhousetraces]
    logs:
      receivers: [otlp]
      processors: [batch]
      exporters: [clickhouselogsexporter]
```

> **Note**: This is a minimal starting config. SigNoz's collector image ships with a
> built-in default config that handles all the ClickHouse routing, metrics pipelines,
> and span-metrics generation. We override only to lock down a known-good config. If
> this causes issues, remove the volume mount in docker-compose.signoz.yml and let the
> image use its defaults.

- [ ] **Step 2: Create the SigNoz Docker Compose file**

Create `docker-compose.signoz.yml` at the project root. Reference SigNoz's official
compose at `https://github.com/SigNoz/signoz/blob/main/deploy/docker/docker-compose.yaml`
for exact image tags and health checks.

```yaml
# SigNoz observability stack (optional — start separately from core sidecars)
# Usage: docker compose -f docker-compose.signoz.yml up -d
# UI: http://localhost:3301

x-clickhouse-defaults: &clickhouse-defaults
  image: clickhouse/clickhouse-server:25.5.6
  restart: unless-stopped
  tty: true
  logging:
    options:
      max-size: 50m
      max-file: "3"
  ulimits:
    nproc: 65535
    nofile:
      soft: 262144
      hard: 262144
  healthcheck:
    test: ["CMD", "wget", "--spider", "-q", "localhost:8123/ping"]
    interval: 30s
    timeout: 5s
    retries: 3

services:
  init-clickhouse:
    <<: *clickhouse-defaults
    hostname: clickhouse
    command:
      - /bin/sh
      - -c
      - |
        # Download histogram quantile binary if not present
        if [ ! -f /var/lib/clickhouse/user_scripts/histogramQuantile ]; then
          mkdir -p /var/lib/clickhouse/user_scripts
          wget -q "https://github.com/SigNoz/signoz/releases/download/v0.116.1/histogramQuantile" \
            -O /var/lib/clickhouse/user_scripts/histogramQuantile
          chmod +x /var/lib/clickhouse/user_scripts/histogramQuantile
        fi
    volumes:
      - signoz-clickhouse:/var/lib/clickhouse
    restart: "no"

  zookeeper:
    image: signoz/zookeeper:3.7.1
    restart: unless-stopped
    volumes:
      - signoz-zookeeper:/data
      - signoz-zookeeper-datalog:/datalog
      - signoz-zookeeper-logs:/logs
    healthcheck:
      test: ["CMD", "bash", "-c", "echo ruok | nc localhost 2181"]
      interval: 10s
      timeout: 5s
      retries: 10

  clickhouse:
    <<: *clickhouse-defaults
    hostname: clickhouse
    ports:
      - "127.0.0.1:9000:9000"
      - "127.0.0.1:8123:8123"
    volumes:
      - signoz-clickhouse:/var/lib/clickhouse
    depends_on:
      zookeeper:
        condition: service_healthy

  signoz:
    image: signoz/signoz:v0.116.1
    restart: unless-stopped
    ports:
      - "127.0.0.1:3301:8080"
    environment:
      - SIGNOZ_CLICKHOUSE_HOST=clickhouse
      - SIGNOZ_CLICKHOUSE_PORT=9000
      - SIGNOZ_SQLITE_PATH=/var/lib/signoz/signoz.db
    volumes:
      - signoz-sqlite:/var/lib/signoz
    depends_on:
      clickhouse:
        condition: service_healthy

  signoz-telemetrystore-migrator:
    image: signoz/signoz-otel-collector:v0.144.2
    command:
      - "--config=/etc/otel/otel-collector-config.yaml"
      - "--manager-config=/etc/manager/config.yaml"
      - "--copy-and-shutdown"
    environment:
      - SIGNOZ_CLICKHOUSE_HOST=clickhouse
      - SIGNOZ_CLICKHOUSE_PORT=9000
    depends_on:
      clickhouse:
        condition: service_healthy
    restart: "no"

  otel-collector:
    image: signoz/signoz-otel-collector:v0.144.2
    restart: unless-stopped
    ports:
      - "127.0.0.1:4317:4317"    # OTLP gRPC
      - "127.0.0.1:4318:4318"    # OTLP HTTP
    environment:
      - SIGNOZ_CLICKHOUSE_HOST=clickhouse
      - SIGNOZ_CLICKHOUSE_PORT=9000
    volumes:
      - ./deploy/common/signoz/otel-collector-config.yaml:/etc/otel/otel-collector-config.yaml:ro
    command: ["--config", "/etc/otel/otel-collector-config.yaml"]
    depends_on:
      clickhouse:
        condition: service_healthy
      signoz-telemetrystore-migrator:
        condition: service_completed_successfully

volumes:
  signoz-clickhouse:
  signoz-zookeeper:
  signoz-zookeeper-datalog:
  signoz-zookeeper-logs:
  signoz-sqlite:
```

> **Implementation note**: The `init-clickhouse` and `signoz-telemetrystore-migrator`
> services are required for first boot — they set up ClickHouse schemas and download
> the histogram quantile UDF binary. Without them, the otel-collector will fail to
> export because `signoz_traces`/`signoz_logs` databases won't exist. If the migrator
> command flags don't match the image version, check the SigNoz official compose at
> `https://github.com/SigNoz/signoz/blob/main/deploy/docker/docker-compose.yaml`.

- [ ] **Step 3: Validate the compose file**

Run: `docker compose -f docker-compose.signoz.yml config`

Expected: YAML parsed successfully, no errors. Don't start the stack yet — we'll
test end-to-end after the Rust changes are done.

- [ ] **Step 4: Commit**

```bash
git add docker-compose.signoz.yml deploy/common/signoz/
git commit -m "feat: add SigNoz Docker Compose stack for self-hosted observability"
```

---

## Task 2: Swap Cargo.toml Dependencies

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: Update dependencies**

In `Cargo.toml`, remove `logfire` and add the OpenTelemetry stack:

Remove line 29:
```toml
logfire = "0.9"
```

Add in the `[dependencies]` section (alphabetical placement):
```toml
opentelemetry = "0.31"
opentelemetry_sdk = { version = "0.31", features = ["rt-tokio"] }
opentelemetry-otlp = { version = "0.31", default-features = false, features = [
  "trace",
  "http-proto",
  "reqwest-client",
  "reqwest-rustls",
] }
tracing-opentelemetry = "0.32"
```

> **Why `default-features = false`**: The `opentelemetry-otlp` crate defaults to
> `grpc-tonic` + `reqwest-blocking-client`. We disable those and explicitly enable only
> `http-proto` + `reqwest-client` (async) + `reqwest-rustls`. This avoids pulling in
> `tonic`/`prost`/`h2` (the full gRPC stack). Ghost already has `reqwest` with `rustls`.
>
> **Before writing code**: Verify the exact feature flag names against `opentelemetry-otlp`
> 0.31's `Cargo.toml` using `@context7`. Feature names have changed between versions
> (e.g., `reqwest-client` vs `reqwest`, `http-proto` vs `http`). Wrong names will either
> fail to compile or silently pull in the wrong transport.

- [ ] **Step 2: Verify dependencies resolve**

Run: `cargo check 2>&1 | head -30`

Expected: Dependency resolution succeeds (new crates download), but compilation fails
with errors in `src/observability.rs` about missing `logfire` crate. That's expected —
we fix it in the next task.

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "chore: swap logfire for opentelemetry-otlp + tracing-opentelemetry"
```

---

## Task 3: Rewrite `src/observability.rs`

**Files:**
- Rewrite: `src/observability.rs`

This is the critical task. The file goes from 154 lines (logfire-based) to ~155 lines
(standard OTel). The public API stays the same: `init()`, `init_for_live_tests()`,
`DaemonObservability`, `ObservabilityError`.

- [ ] **Step 1: Write the new observability.rs**

Replace the entire file with:

```rust
use std::sync::OnceLock;

use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::SpanExporter;
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_sdk::Resource;
use thiserror::Error;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, EnvFilter, Layer};

/// Holds the tracer provider for the test process so the export pipeline
/// stays alive until process exit. Initialized exactly once.
static TEST_PROVIDER: OnceLock<SdkTracerProvider> = OnceLock::new();

#[derive(Debug, Error)]
pub enum ObservabilityError {
    #[error("failed to initialize tracer: {0}")]
    TracerInit(#[from] opentelemetry_sdk::trace::TraceError),
}

pub struct DaemonObservability {
    _tracer_provider: Option<SdkTracerProvider>,
}

impl DaemonObservability {
    fn disabled() -> Self {
        Self {
            _tracer_provider: None,
        }
    }
}

impl Drop for DaemonObservability {
    fn drop(&mut self) {
        if let Some(provider) = self._tracer_provider.take() {
            if let Err(e) = provider.shutdown() {
                eprintln!("failed to shut down tracer provider: {e}");
            }
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

    let env_filter = EnvFilter::from_default_env();
    let fmt_layer = fmt::layer()
        .with_ansi(true)
        .with_timer(fmt::time::SystemTime);

    let provider = build_tracer_provider("production")?;

    if let Some(ref provider) = provider {
        let otel_layer = tracing_opentelemetry::layer()
            .with_tracer(provider.tracer("ghost"))
            .with_filter(tracing_subscriber::filter::LevelFilter::INFO);

        tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt_layer)
            .with(otel_layer)
            .init();
    } else {
        tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt_layer)
            .init();
    }

    Ok(DaemonObservability {
        _tracer_provider: provider,
    })
}

pub fn init_for_live_tests() -> Result<DaemonObservability, ObservabilityError> {
    static INIT: std::sync::Once = std::sync::Once::new();

    let mut result: Option<ObservabilityError> = None;
    INIT.call_once(|| {
        crate::config::load_dotenv_from_config_dir();
        set_default_rust_log_filter_for_tests();

        let env_filter = EnvFilter::from_default_env();
        let fmt_layer = fmt::layer()
            .with_ansi(true)
            .without_time()
            .compact();

        match build_tracer_provider("test") {
            Ok(Some(provider)) => {
                let otel_layer = tracing_opentelemetry::layer()
                    .with_tracer(provider.tracer("ghost"))
                    .with_filter(tracing_subscriber::filter::LevelFilter::INFO);

                tracing_subscriber::registry()
                    .with(env_filter)
                    .with(fmt_layer)
                    .with(otel_layer)
                    .init();

                let _ = TEST_PROVIDER.set(provider);
            }
            Ok(None) => {
                tracing_subscriber::registry()
                    .with(env_filter)
                    .with(fmt_layer)
                    .init();
            }
            Err(e) => {
                result = Some(e);
            }
        }
    });

    if let Some(error) = result {
        return Err(error);
    }

    Ok(DaemonObservability::disabled())
}

/// Build a tracer provider with OTLP export if `OTEL_EXPORTER_OTLP_ENDPOINT` is set.
/// Returns `None` if no endpoint is configured (console-only mode).
fn build_tracer_provider(
    environment: &str,
) -> Result<Option<SdkTracerProvider>, ObservabilityError> {
    if std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").is_err() {
        return Ok(None);
    }

    let exporter = SpanExporter::builder()
        .with_http()
        .build()
        .map_err(ObservabilityError::TracerInit)?;

    let service_name = std::env::var("OTEL_SERVICE_NAME")
        .unwrap_or_else(|_| "GHOST".to_string());

    let resource = Resource::builder()
        .with_service_name(service_name)
        .with_attribute(opentelemetry::KeyValue::new(
            "deployment.environment.name",
            environment.to_string(),
        ))
        .build();

    let provider = SdkTracerProvider::builder()
        .with_resource(resource)
        .with_batch_exporter(exporter)
        .build();

    opentelemetry::global::set_tracer_provider(provider.clone());

    Ok(Some(provider))
}

fn install_panic_handler() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // Edition 2024: use payload_as_str() if available, fall back to
        // downcast for older toolchains. Check if message() or
        // payload_as_str() compiles — if not, use the downcast pattern.
        let message = info
            .payload_as_str()
            .unwrap_or("unknown panic");
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "unknown".to_string());

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
```

> **Key differences from old code:**
> - `logfire::configure()` → `SdkTracerProvider::builder()` + `tracing_subscriber::registry()`
> - `logfire::ShutdownGuard` → `SdkTracerProvider` held in struct, `shutdown()` on Drop
> - `logfire::config::ConsoleOptions` → `tracing_subscriber::fmt` layer
> - `set_default_logfire_environment()` → removed (replaced by resource attribute)
> - `logfire::config::SendToLogfire::IfTokenPresent` → `OTEL_EXPORTER_OTLP_ENDPOINT` env check
> - Panic handler: `with_install_panic_handler(true)` → explicit `std::panic::set_hook`
> - The `build_tracer_provider()` helper DRYs the shared logic between `init()` and
>   `init_for_live_tests()`

- [ ] **Step 2: Verify the module compiles in isolation**

Run: `cargo check 2>&1 | head -50`

Expected: `src/observability.rs` compiles. Remaining errors are all in other files
referencing `logfire::info!`, `logfire::warn!`, etc. — those are fixed in Task 4.

> **If `SpanExporter::builder().with_http().build()` doesn't compile**: The HTTP
> builder method name may differ. Check the docs with `@context7` for
> `opentelemetry-otlp` 0.31. The builder might need `.with_http_client(reqwest::Client::new())`
> or the feature flag might auto-select the transport. Consult
> `opentelemetry_otlp::SpanExporter` docs.

- [ ] **Step 3: Commit**

```bash
git add src/observability.rs
git commit -m "feat: rewrite observability pipeline — logfire → standard OpenTelemetry"
```

---

## Task 4: Macro Migration (159 Call Sites, 33 Files)

**Files:**
- Modify: 33 files in `src/` (see file list in spec section 4)

This is a mechanical find-and-replace. All `logfire::` macros map 1:1 to `tracing::`
macros. The `#[tracing::instrument]` decorations are already standard and need zero
changes.

- [ ] **Step 1: Replace all `logfire::` macros with `tracing::` equivalents**

Run these replacements across all files in `src/`:

| Find | Replace |
|------|---------|
| `logfire::info!` | `tracing::info!` |
| `logfire::warn!` | `tracing::warn!` |
| `logfire::error!` | `tracing::error!` |
| `logfire::debug!` | `tracing::debug!` |
| `logfire::span!` | `tracing::info_span!` |

There is exactly **1** `logfire::span!` call site in `src/daemon/watcher.rs:114`.
The rest are info/warn/error/debug.

> **Important**: After replacing `logfire::span!` with `tracing::info_span!`, verify
> that the `.instrument()` call on the same line still works. `tracing::info_span!`
> returns a `tracing::Span` which implements `Instrument`, so
> `.instrument(tracing::info_span!(...))` is the correct pattern.

- [ ] **Step 2: Remove stale `use logfire` imports**

After replacing the macros, some files may still have `use logfire::...` imports. There
should be none outside `observability.rs` (which was already rewritten), but verify:

Search for any remaining `logfire` references in `src/`:

```bash
grep -r "logfire" src/
```

Expected: Zero matches. If any remain, remove them.

- [ ] **Step 3: Verify compilation**

Run: `cargo check`

Expected: Clean compilation with zero errors. All `logfire::` references are gone.

If there are errors:
- **"unresolved import `logfire`"** — a file still has a `use logfire::...` line. Remove it.
- **"cannot find macro `logfire`"** — a call site was missed. Replace it.
- **Type mismatch on `.instrument()`** — the `logfire::span!` replacement may need
  adjustment. `tracing::info_span!` returns `tracing::Span`; ensure the call site uses
  `.instrument(tracing::info_span!(...))`.

- [ ] **Step 4: Run tests**

Run: `cargo test`

Expected: All existing tests pass. The macro migration is a 1:1 swap — no behavior
change.

- [ ] **Step 5: Commit**

```bash
git add src/
git commit -m "refactor: migrate 159 logfire:: macro calls to standard tracing::"
```

---

## Task 5: Live Test — OTLP Export to SigNoz

**Files:**
- Modify: `Cargo.toml` (add `live-tests-observability` feature)
- Create: `tests/observability_live.rs`

This test verifies the full OTLP pipeline end-to-end: Ghost → OTel collector → SigNoz.
It runs against a local SigNoz instance started via Docker Compose.

- [ ] **Step 1: Add feature flag**

In `Cargo.toml`, add to the `[features]` section:

```toml
live-tests-observability = ["live-tests"]
```

- [ ] **Step 2: Start SigNoz stack**

```bash
docker compose -f docker-compose.signoz.yml up -d
```

Wait for the collector to be healthy:

```bash
# Poll health endpoint (max ~60s)
until curl -sf http://localhost:13133/; do sleep 2; done
echo "OTel collector healthy"
```

- [ ] **Step 3: Write the live test**

Create `tests/observability_live.rs`:

```rust
//! Live test: verify OTLP export reaches a local SigNoz instance.
//!
//! Prerequisites:
//!   docker compose -f docker-compose.signoz.yml up -d
//!
//! Run:
//!   cargo test --features live-tests-observability --test observability_live

#![cfg(feature = "live-tests-observability")]

use std::time::Duration;

use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::SpanExporter;
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_sdk::Resource;

/// Verify that spans exported via OTLP HTTP reach the local SigNoz instance.
///
/// Strategy:
/// 1. Build a tracer provider pointing at localhost:4318 (HTTP OTLP)
/// 2. Create a span with a unique, identifiable service name
/// 3. Shut down the provider (flushes the batch exporter)
/// 4. Query the SigNoz API for that service name
/// 5. Assert the service appears in the response
#[tokio::test]
async fn otlp_spans_reach_signoz() {
    // Use a unique service name so we don't collide with a running Ghost daemon.
    let test_service = format!("ghost-test-{}", ulid::Ulid::new());

    // ── 1. Build a standalone tracer provider (no tracing-subscriber needed) ──
    let exporter = SpanExporter::builder()
        .with_http()
        .build()
        .expect("build OTLP HTTP exporter — is opentelemetry-otlp configured?");

    let resource = Resource::builder()
        .with_service_name(test_service.clone())
        .build();

    let provider = SdkTracerProvider::builder()
        .with_resource(resource)
        .with_batch_exporter(exporter)
        .build();

    // ── 2. Create a span ──
    let tracer = provider.tracer("observability-live-test");
    {
        use opentelemetry::trace::Tracer;
        tracer.in_span("test verify_otlp", |_cx| {
            // Span body — nothing needed, existence is the test.
        });
    }

    // ── 3. Flush ──
    provider
        .shutdown()
        .expect("shutdown tracer provider (flushes spans)");

    // Give SigNoz a moment to ingest.
    tokio::time::sleep(Duration::from_secs(3)).await;

    // ── 4. Query SigNoz API for the service ──
    let now_nanos = chrono::Utc::now().timestamp_nanos_opt().unwrap();
    let five_min_ago = now_nanos - 5 * 60 * 1_000_000_000;

    let url = format!(
        "http://localhost:3301/api/v1/services?start={five_min_ago}&end={now_nanos}"
    );

    let client = reqwest::Client::new();
    let resp = client
        .get(&url)
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .expect("query SigNoz services API — is SigNoz running on :3301?");

    let body: serde_json::Value = resp
        .json()
        .await
        .expect("parse SigNoz services response as JSON");

    // SigNoz returns an array of service objects. Find ours.
    let services = body
        .as_array()
        .expect("SigNoz services response should be an array");

    let found = services.iter().any(|svc| {
        svc.get("serviceName")
            .and_then(|v| v.as_str())
            .map(|name| name == test_service)
            .unwrap_or(false)
    });

    assert!(
        found,
        "service '{}' not found in SigNoz. Got: {:?}",
        test_service,
        services
            .iter()
            .filter_map(|s| s.get("serviceName").and_then(|v| v.as_str()))
            .collect::<Vec<_>>()
    );
}

/// Verify that when OTEL_EXPORTER_OTLP_ENDPOINT is unset, the pipeline
/// initializes without errors and no connection is attempted.
#[tokio::test]
async fn console_only_mode_no_errors() {
    // Ensure OTLP endpoint is NOT set for this test.
    // Save and restore if it was set.
    let prev = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").ok();
    std::env::remove_var("OTEL_EXPORTER_OTLP_ENDPOINT");

    // init_for_live_tests is already called once per process, so we test
    // build_tracer_provider indirectly: just verify no panic/error on init.
    // The real assertion is that this doesn't hang or fail trying to connect.
    let result = ghost::observability::init_for_live_tests();
    assert!(result.is_ok(), "init_for_live_tests failed: {:?}", result.err());

    // Restore
    if let Some(val) = prev {
        std::env::set_var("OTEL_EXPORTER_OTLP_ENDPOINT", val);
    }
}
```

> **Note on the SigNoz API**: The `/api/v1/services` endpoint returns services that
> have sent traces. The exact response format may vary by SigNoz version. If the API
> shape is different, check `curl http://localhost:3301/api/v1/services?start=0&end=999999999999999999`
> to see the actual format and adjust the assertion.
>
> **If the test can't find the service**: The batch exporter has a default flush
> interval. `provider.shutdown()` should force a flush, but if SigNoz is slow to
> ingest, increase the sleep duration. Also check that the collector is healthy
> (`curl http://localhost:13133/`).

- [ ] **Step 4: Run the test**

```bash
# Ensure SigNoz is running
docker compose -f docker-compose.signoz.yml ps

# Run the live test
OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318 \
  cargo test --features live-tests-observability --test observability_live -- --nocapture
```

Expected: Both tests pass. `otlp_spans_reach_signoz` finds the test service in SigNoz.
`console_only_mode_no_errors` completes without errors.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml tests/observability_live.rs
git commit -m "test: add live test for OTLP export to SigNoz"
```

- [ ] **Step 6: Stop SigNoz (optional, for CI cleanup)**

```bash
docker compose -f docker-compose.signoz.yml down
```

---

## Task 6: Update Documentation and Skills

**Files:**
- Modify: `.agents/skills/tracing/SKILL.md`
- Delete: `.agents/skills/logfire/SKILL.md`
- Modify: `CLAUDE.md` and `AGENTS.md` (identical content)
- Modify: `.claude/settings.local.json`
- Modify: `deploy/common/onboard.py`

- [ ] **Step 1: Update the tracing skill**

In `.agents/skills/tracing/SKILL.md`, make these changes:

1. **Frontmatter description** (line 6): Replace `logfire::span!() calls` with
   `tracing::info_span!() calls`

2. **Line 106**: Replace:
   ```
   - Use `logfire::span!("verb object", key = val)` for programmatic spans
   ```
   With:
   ```
   - Use `tracing::info_span!("verb object", key = val)` for programmatic spans
   ```

3. **Line 109**: Replace:
   ```
   - Use `logfire::info!()` / `warn!()` / `error!()` for discrete events within spans
   ```
   With:
   ```
   - Use `tracing::info!()` / `warn!()` / `error!()` for discrete events within spans
   ```

4. **Line 115**: Replace:
   ```
   - Default RUST_LOG: `warn,ghost=info,usvg=off,resvg=off`
   ```
   With:
   ```
   - Default RUST_LOG: `warn,ghost=info,chromiumoxide=error,usvg=off,resvg=off,fontdb=off,html5ever=off`
   - OTLP export: conditional on `OTEL_EXPORTER_OTLP_ENDPOINT` env var
   ```

- [ ] **Step 2: Delete the logfire skill**

```bash
rm .agents/skills/logfire/SKILL.md
rmdir .agents/skills/logfire/
```

Verify no other files reference it. The `/logfire` skill name in the skills registry
should be auto-discovered from the filesystem, so deleting the file is sufficient.

- [ ] **Step 3: Update CLAUDE.md and AGENTS.md dependencies**

Both files have identical content. In each, find the line (around line 115):
```
logfire +
```
Replace with:
```
opentelemetry + tracing-opentelemetry +
```

- [ ] **Step 4: Clean up settings.local.json**

In `.claude/settings.local.json`, remove these 7 lines from the `allow` array:
```json
"mcp__logfire__token_info",
"mcp__logfire__query_schema_reference",
"mcp__logfire__project_list",
"mcp__logfire__query_run",
"mcp__logfire__dashboard_list",
"mcp__logfire__alert_list",
"mcp__logfire__project_logfire_link",
```

- [ ] **Step 5: Add OTLP endpoint to onboarding script**

In `deploy/common/onboard.py`, add `OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318`
to the generated `.env` file. Find where other env vars are written (search for
`write_env` or `.env`) and add the OTLP endpoint alongside them. This ensures Ghost
sends traces to SigNoz out of the box for users who run the onboarding wizard.

- [ ] **Step 6: Commit**

```bash
git add .agents/skills/ CLAUDE.md AGENTS.md .claude/settings.local.json deploy/common/onboard.py
git commit -m "docs: update skills and config for logfire → opentelemetry migration"
```

---

## Task 7: Final Verification

- [ ] **Step 1: Run full CI**

Run: `just ci`

This runs `cargo fmt --check`, `cargo check`, `cargo clippy`, and `cargo test`.
All must pass.

Expected: Clean pass. If clippy warns about anything in the new `observability.rs`
(unused imports, redundant clones, etc.), fix them.

- [ ] **Step 2: Run live test against SigNoz (if not done in Task 5)**

```bash
docker compose -f docker-compose.signoz.yml up -d
until curl -sf http://localhost:13133/; do sleep 2; done

OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318 \
  cargo test --features live-tests-observability --test observability_live -- --nocapture

docker compose -f docker-compose.signoz.yml down
```

Expected: Both tests pass — `otlp_spans_reach_signoz` and `console_only_mode_no_errors`.

- [ ] **Step 3: Final commit (if any fixes were needed)**

```bash
git add -A
git commit -m "fix: address CI issues from opentelemetry migration"
```
