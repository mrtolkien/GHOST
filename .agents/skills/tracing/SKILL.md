---
name: tracing
description: >-
  Tracing, observability, and instrumentation conventions for the Ghost codebase. MUST
  READ before adding or modifying: tracing spans, #[tracing::instrument] attributes,
  logfire::span!() calls, OTel semantic convention fields (gen_ai.*), span hierarchy, or
  any observability-related code. Failure to follow these conventions produces
  inconsistent telemetry.
---

# Observability Conventions (NON-NEGOTIABLE)

Instrument meaningful execution boundaries, not every function. Prioritize: provider API
calls, tool executions, agent/job entry points, DB mutations, and embedding ops. Keep
small helpers and pure functions uninstrumented.

## Span Naming — `{verb} {object}`, Low Cardinality

Source: https://opentelemetry.io/blog/2025/how-to-name-your-spans/

Follow the OTel-recommended **`{verb} {object}`** pattern for all span names. The verb
describes the action; the object describes what is acted upon. This keeps span names
**low-cardinality** (aggregatable for P95 latency, dashboards, alerts).

### Rules

1. **Never embed run-dependent values in span names.** IDs, file paths, query strings,
   user names, and tool names belong in **attributes**, not the span name.
2. **Name the operation, not the outcome.** Use `validate input` — not
   `validation_failed`. Record the outcome in span status or an attribute.
3. **Keep names short and stable.** Two words is ideal. Three is the max.

### Current span names

| Span Name              | Attributes (run-dependent)                    |
| ---------------------- | --------------------------------------------- |
| `boot ghost`           | —                                             |
| `receive message`      | `author`, `channel_id`, `content_len`         |
| `process message`      | `session_id`                                  |
| `run agent`            | `gen_ai.agent.name`, `gen_ai.agent.id`        |
| `continue agent`       | `gen_ai.agent.name`, `gen_ai.agent.id`        |
| `start agent`          | `gen_ai.agent.name`                           |
| `execute job`          | `job_name`, `model`                           |
| `run heartbeat`        | `session_id`, `interface`                     |
| `run reflection`       | `session_id`, `agent_name`                    |
| `request completion`   | `gen_ai.system`, `gen_ai.request.model`, etc. |
| `execute tool`         | `gen_ai.tool.name`, `params_preview`          |
| `embed source`         | `source_table`, `source_id`                   |
| `embed batch`          | `model`, `batch_size`                         |
| `reconcile embeddings` | `embedded`, `skipped`                         |
| `search web`           | `query`                                       |
| `fetch url`            | `url`                                         |
| `process file_change`  | `kind`, `path`                                |
| `send message`         | `channel_id`, `content_len`                   |
| `reboot session`       | `old_session_id`                              |

### Bad vs Good

| Bad (high cardinality)      | Good                  |
| --------------------------- | --------------------- |
| `tool: note_write`          | `execute tool`        |
| `watcher: note`             | `process file_change` |
| `agent: research-agent`     | `run agent`           |
| `openai_compatible.request` | `request completion`  |
| `ghost.startup`             | `boot ghost`          |

## OTel GenAI Semantic Conventions (Mandatory for Provider Calls)

Source: https://opentelemetry.io/docs/specs/semconv/registry/attributes/gen-ai/

All LLM provider calls must record these fields on the `request completion` span:

- `gen_ai.system` — provider name (e.g. `openrouter`)
- `gen_ai.operation.name` — always `"chat"`
- `gen_ai.request.model` — model ID sent in the request
- `gen_ai.response.model` — model ID returned by the provider
- `gen_ai.response.id` — response identifier
- `gen_ai.response.finish_reasons` — stop reason
- `gen_ai.usage.input_tokens` / `gen_ai.usage.output_tokens`
- `gen_ai.usage.cache_read_input_tokens` / `gen_ai.usage.cache_creation_input_tokens`

## Span Hierarchy — Keep It Shallow (2-3 Levels Max)

- `receive message` -> `request completion` / `execute tool`
- `run agent` -> `request completion` / `execute tool`
- `run reflection` -> `run agent` -> `request completion` / `execute tool`
- Never wrap a single delegation call in a span

## Implementation Rules

- Use `#[tracing::instrument(name = "verb object", skip_all, fields(key = %val))]` —
  always set an explicit `name` on info-level spans at execution boundaries
- Use `logfire::span!("verb object", key = val)` for programmatic spans — **never
  interpolate dynamic values into the span name string**
- Use `tracing::Span::current().record()` to fill response fields at span completion
- Use `logfire::info!()` / `warn!()` / `error!()` for discrete events within spans
- Do not duplicate information between a span and a child log event
- Log all errors with full context before propagating
- Provider request/response bodies: logged as `gen_ai.input.messages` /
  `gen_ai.output.messages` at info level inside `request completion` spans
- Debug-level DB/internal spans may omit `name =` (function name default is fine)
- Default RUST_LOG: `warn,ghost=info,usvg=off,resvg=off`
