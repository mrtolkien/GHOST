We should likely use an open source opentelemetry system!

This way it could be part of the default Docker stack

Could be Signoz?

---

# Design: Self-Hosted OpenTelemetry with SigNoz

## Goal

Replace the `logfire` crate with standard OpenTelemetry OTLP export and add SigNoz to
the Docker Compose stack as a self-hosted observability backend. The result is a fully
self-hosted telemetry pipeline with zero cloud dependencies, configurable via standard
OTel environment variables for users who want to point at a different backend.

## Scope

- Add SigNoz services to a **separate** Docker Compose file
- Replace `logfire` Rust crate with `opentelemetry-otlp` + `tracing-opentelemetry`
- Migrate 159 `logfire::` macro call sites across 33 files to standard `tracing::`
  macros
- All 217+ `#[tracing::instrument]` decorations remain untouched
- Update/remove stale logfire-related skills and MCP config

**Out of scope**: Tailscale (deferred to onboarding work), custom dashboards, alerting
setup, metrics (traces only for now, logs stay console-only).

## 1. SigNoz Docker Compose Stack

New file: `docker-compose.signoz.yml`

Uses SigNoz's official images (v0.116+). Services:

| Service                        | Image                             | Purpose                | Exposed Port             |
| ------------------------------ | --------------------------------- | ---------------------- | ------------------------ |
| zookeeper                      | signoz/zookeeper:3.7.1            | ClickHouse coordinator | — (internal)             |
| clickhouse                     | clickhouse/clickhouse-server:25.x | Trace/log storage      | — (internal)             |
| init-clickhouse                | clickhouse/clickhouse-server:25.x | One-time DB setup      | — (init container)       |
| signoz-telemetrystore-migrator | signoz/signoz-otel-collector      | Schema migrations      | — (init container)       |
| signoz                         | signoz/signoz                     | Query service + Web UI | 3301                     |
| otel-collector                 | signoz/signoz-otel-collector      | OTLP ingestion         | 4317 (gRPC), 4318 (HTTP) |

That's 6 services (4 persistent + 2 init containers). SigNoz merged query-service and
frontend into a single `signoz/signoz` image. ZooKeeper is still required for ClickHouse
coordination. Minimum **4 GB RAM** allocated to Docker.

Services expose ports on localhost via `ports:` mappings — no shared Docker network
needed. Ghost (running natively) sends OTLP to `localhost:4317`/`4318`.

**Usage**:

```bash
# Start observability stack
docker compose -f docker-compose.signoz.yml up -d

# Start core sidecar services (unchanged)
docker compose up -d
```

The two compose files are independent — SigNoz is optional.

## 2. Rust Dependency Changes

### Remove

- `logfire = "0.9"`

### Add

- `opentelemetry = "0.31"` — core OTel API
- `opentelemetry_sdk = { version = "0.31", features = ["rt-tokio"] }` — SDK with async
  batch processor
- `opentelemetry-otlp = { version = "0.31", features = ["http-proto", "reqwest-rustls"] }`
  — OTLP exporter using HTTP + reqwest with rustls (matches project's TLS story, avoids
  adding `tonic`/gRPC dependency)
- `tracing-opentelemetry = "0.32"` — bridges `tracing` spans to OTel spans

### Keep

- `tracing = "0.1"` (unchanged)
- `tracing-subscriber = "0.3"` (unchanged)

Using the HTTP/protobuf OTLP exporter with reqwest (already a dependency) to avoid
pulling in `tonic` and the gRPC stack.

## 3. Pipeline Setup (`src/observability.rs`)

Replace `logfire::configure()` with a manual two-layer pipeline:

### Console Layer (always active)

```
tracing_subscriber::fmt layer
  → timestamps, ANSI color, target display
  → filtered by RUST_LOG (same as today)
```

### OTLP Layer (conditional)

Only installed when `OTEL_EXPORTER_OTLP_ENDPOINT` is set:

```
tracing-opentelemetry layer
  → filtered at INFO+ (no debug spans exported, avoids flooding collector)
  → opentelemetry_sdk BatchSpanProcessor
  → opentelemetry-otlp HTTP exporter
  → resource: service.name = OTEL_SERVICE_NAME or "GHOST"
```

**Traces only** — log events stay console-only for now. If log export to SigNoz is
desired later, add `opentelemetry-appender-tracing` as a log bridge.

### Shutdown

Return an `ObservabilityGuard` struct that calls
`opentelemetry::global::shutdown_tracer_provider()` on `Drop`. The caller holds this
guard for the lifetime of the process (same pattern as logfire's `ShutdownGuard`).

### Panic Handler

Install a `std::panic::set_hook` that logs panics via `tracing::error!` with
`panic.message` and `panic.location` fields. Replaces logfire's
`with_install_panic_handler(true)`.

### Test Initialization

`init_for_live_tests()` sets `service.name = "GHOST"` with an additional resource
attribute `deployment.environment = "test"`. Console output uses a test-friendly format
(no timestamps, compact). No OTLP export in tests unless explicitly configured.

## 4. Macro Migration

Direct 1:1 replacements — 159 call sites across 33 files (70 warn, 53 info, 27 error, 8
debug, 1 span). Heaviest files: `agents/runner.rs` (19), `daemon/watcher.rs` (16),
`chat/compaction.rs` (15), `agents/scheduler.rs` (12), `web/curation.rs` (11):

| Before                            | After                                  |
| --------------------------------- | -------------------------------------- |
| `logfire::span!(name, fields...)` | `tracing::info_span!(name, fields...)` |
| `logfire::info!(msg, fields...)`  | `tracing::info!(msg, fields...)`       |
| `logfire::warn!(msg, fields...)`  | `tracing::warn!(msg, fields...)`       |
| `logfire::error!(msg, fields...)` | `tracing::error!(msg, fields...)`      |
| `logfire::debug!(msg, fields...)` | `tracing::debug!(msg, fields...)`      |

The `#[tracing::instrument(...)]` decorations (217+) are already standard tracing and
require **zero changes**.

## 5. Environment Variables

All standard OpenTelemetry env vars — no custom config keys:

| Variable                      | Default             | Purpose                                     |
| ----------------------------- | ------------------- | ------------------------------------------- |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | (unset = no export) | Collector URL, e.g. `http://localhost:4318` |
| `OTEL_EXPORTER_OTLP_PROTOCOL` | `http/protobuf`     | Transport protocol                          |
| `OTEL_SERVICE_NAME`           | `GHOST`             | Service identifier in traces                |
| `RUST_LOG`                    | (existing filter)   | Console + export filtering (unchanged)      |

The existing `set_default_rust_log_filter()` logic is preserved as-is — it sets a
multi-target default (`warn,ghost=info,chromiumoxide=error,...`), not a naive `info`.

No `config.toml` changes. The install/onboarding script sets
`OTEL_EXPORTER_OTLP_ENDPOINT` in the `.env` file when SigNoz is running.

## 6. What Breaks / What to Update

- **Logfire MCP tools**: Stop working (no Logfire cloud). SigNoz has its own web UI for
  querying traces. Users who want Logfire cloud can point `OTEL_EXPORTER_OTLP_ENDPOINT`
  at Logfire's OTLP endpoint and set
  `OTEL_EXPORTER_OTLP_HEADERS="Authorization=Bearer <token>"` for auth.
- **Tracing skill** (`.agents/skills/tracing/`): Update to remove logfire-specific
  references (macro names, configure call). Core conventions (span naming, hierarchy,
  instrumentation scope) stay the same.
- **MEMORY.md**: Remove logfire-specific entries, add SigNoz/OTel notes.
- **Onboarding** (`deploy/common/onboard.py`): Add `OTEL_EXPORTER_OTLP_ENDPOINT` to
  generated `.env` when SigNoz compose is used.
- **Error types**: Rename `ObservabilityError::LogfireInit` to `TracerInit`. Remove
  `set_default_logfire_environment()` function.
- **CLAUDE.md**: Update dependencies list (replace `logfire` with
  `opentelemetry + tracing-opentelemetry`).
- **`.claude/settings.local.json`**: Remove stale `mcp__logfire__*` permission entries.

## 7. GenAI Semantic Conventions

All `gen_ai.*` fields on `request completion` spans are standard OpenTelemetry semantic
conventions — they work identically with SigNoz. SigNoz's LLM observability features
consume these fields natively. No changes needed to provider instrumentation.
