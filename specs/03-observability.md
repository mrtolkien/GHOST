# 03 — Observability Setup (Logfire + Tracing)

## Overview

Observability is non-negotiable. Every meaningful operation gets a tracing span. Logfire
provides the dashboard and storage. The `tracing` crate provides the instrumentation
API.

## Architecture

```
Application code
    ↓ (tracing macros / #[instrument])
tracing subscriber
    ↓
logfire exporter (OpenTelemetry)
    ↓
Logfire dashboard (cloud)
```

Logfire-rust integrates with the `tracing` ecosystem. Anything instrumented with
`tracing` automatically flows to logfire. The `log` crate output is also captured.

## Setup

```rust
// src/observability/mod.rs

pub fn init() {
    // Initialize logfire — reads LOGFIRE_TOKEN from env
    // Falls back to console-only tracing if no token
    logfire::configure()
        .service_name("ghost")
        .install_panic_handler()
        .send_to_logfire(true)
        .finish()
        .expect("failed to initialize logfire");
}
```

## Instrumentation Guidelines

### Every public async function:

```rust
#[tracing::instrument(skip_all, fields(session_id = %session_id))]
pub async fn chat(&self, session_id: &str, message: &str) -> Result<Response> {
    // ...
}
```

### Key spans to instrument:

| Area           | Span name             | Key fields                                   |
| -------------- | --------------------- | -------------------------------------------- |
| Provider calls | `provider.chat`       | provider, model, input_tokens, output_tokens |
| Tool execution | `tool.execute`        | tool_name, duration_ms                       |
| Discord        | `discord.message`     | user_id, channel_id                          |
| Job execution  | `job.execute`         | job_name, trigger_type                       |
| Knowledge      | `knowledge.search`    | query, result_count                          |
| Knowledge      | `knowledge.write`     | note_title, archetype                        |
| Embeddings     | `embeddings.generate` | model, batch_size, chunk_count               |
| Compaction     | `session.compact`     | session_id, messages_before, messages_after  |
| Web            | `web.search`          | query, provider, result_count                |
| Web            | `web.fetch`           | url, status_code, content_length             |
| Config         | `config.load`         | config_path                                  |
| DB             | `db.query`            | table, operation                             |

### Structured events for key moments:

```rust
logfire::info!("session started", session_id = %id);
logfire::info!("provider response", model = %model, tokens = input + output);
logfire::warn!("provider rate limited", provider = %name, retry_after = %secs);
logfire::error!("job failed", job_name = %name, error = %e);
```

### Error logging:

Log errors at the boundary where they are handled, with full context:

```rust
if let Err(e) = self.run_job(&job).await {
    logfire::error!("job execution failed",
        job_name = %job.name,
        trigger = %job.trigger,
        error = %e,
    );
}
```

## Environment Variables

```bash
LOGFIRE_TOKEN=...           # Logfire API token (optional — falls back to console)
RUST_LOG=ghost=debug,info   # Standard tracing env filter
```

## Console Fallback

When `LOGFIRE_TOKEN` is not set, tracing output goes to stderr via `tracing-subscriber`
with a human-readable format. This ensures observability works in development without a
logfire account.

## Validation

1. Set `LOGFIRE_TOKEN` in `.env` and run any subcommand — spans appear in the logfire
   dashboard within seconds
2. Run without `LOGFIRE_TOKEN` — structured logs go to stderr, no crash
3. `cargo test` — tracing subscriber initializes without panicking
4. Check the logfire dashboard: spans include service name, environment, and custom
   fields
5. `just ci` — passes

## Acceptance Criteria

- `ghost daemon` initializes logfire on startup
- Console output shows structured logs when no logfire token is set
- Spans flow to logfire dashboard when `LOGFIRE_TOKEN` is set
- Panic handler is installed (panics appear in logfire)
- `RUST_LOG` env filter works for controlling verbosity
- `just ci` passes
