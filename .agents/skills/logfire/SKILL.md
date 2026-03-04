---
name: logfire
description: >-
  How to query Ghost production telemetry via the Logfire MCP. MUST READ when: (1) the
  user asks to debug an issue on a live/running GHOST instance, (2) the user mentions
  Logfire, traces, or production errors, (3) you need to use any mcp__logfire__* tool.
  Covers project setup, span names, query recipes, SQL gotchas, and dashboard/alert
  management.
---

# Logfire MCP — Ghost Project

## Setup

- Token is **org-scoped**, so pass `project: "ghost"` on every call.
- Service name: `"GHOST"`
- Query engine is **Apache DataFusion** (Postgres-like SQL, not actual Postgres).

## Database Schema (Key Tables)

### `records` — Spans and Logs

| Column              | Use                                                    |
| ------------------- | ------------------------------------------------------ |
| `span_name`         | Span type — filter here first                          |
| `message`           | Human-readable (often same as `span_name`)             |
| `attributes`        | JSON — use `->>'key'` for text, `->` + cast            |
| `trace_id`          | Group related spans into a trace                       |
| `span_id`           | Unique span identifier                                 |
| `parent_span_id`    | Parent in the span tree                                |
| `kind`              | `"span"` or `"log"`                                    |
| `level`             | OTel severity: 1=trace 5=debug 9=info 13=warn 17=error |
| `duration`          | Span duration in seconds (float)                       |
| `start_timestamp`   | When the span started                                  |
| `otel_status_code`  | `"ERROR"` for failed spans                             |
| `is_exception`      | Boolean — true if exception event attached             |
| `exception_type`    | Exception class name                                   |
| `exception_message` | Exception detail                                       |
| `service_name`      | `"GHOST"` for this project                             |

### `metrics` — Not currently used by Ghost

### `ai_counts` — Aggregated AI usage counts

## Efficient Filtering

These columns have efficient indexes — always filter on them:

- `start_timestamp` (time range — set via the `age` parameter in minutes)
- `service_name` (use `WHERE service_name = 'GHOST'` if needed)
- `span_name` (most useful filter)
- `trace_id` (for following a single trace)

## Ghost Span Names (What to Query)

| `span_name`                    | What it represents              | Key attributes                          |
| ------------------------------ | ------------------------------- | --------------------------------------- |
| `boot ghost`                   | Daemon startup                  | —                                       |
| `create provider`              | Provider initialization         | `provider`, `endpoint`                  |
| `create session_chat`          | Session chat setup              | —                                       |
| `orchestrate response`         | Chat turn orchestration         | `session_id`                            |
| `request completion`           | LLM provider call               | `gen_ai.*` fields (model, tokens, etc.) |
| `execute agent`                | Full agent run                  | `gen_ai.agent.name`, `gen_ai.agent.id`  |
| `run lua agent`                | Lua agent session               | `gen_ai.agent.name`                     |
| `run lua agent with history`   | Agent with message history      | `gen_ai.agent.name`                     |
| `start agent`                  | Agent spawn                     | `gen_ai.agent.name`                     |
| `resume agent bg`              | Background agent resume         | `agent_id`                              |
| `execute resume`               | Resume execution                | —                                       |
| `receive discord message`      | Incoming Discord message        | `author`, `channel_id`, `content_len`   |
| `send message`                 | Outgoing message                | —                                       |
| `run tool`                     | Single tool execution           | `gen_ai.tool.name` + tool-specific      |
| `run tools`                    | Tool execution batch            | —                                       |
| `embed source`                 | Single embedding                | `source_id`, `source_table`             |
| `embed batch`                  | Batch embedding                 | —                                       |
| `embed sources`                | Embedding pipeline              | —                                       |
| `reconcile embeddings`         | Embedding reconciliation        | —                                       |
| `process file_change`          | File watcher event (single)     | `kind`, `path`                          |
| `process file_changes`         | File watcher events (batch)     | —                                       |
| `reboot session`               | Session reboot                  | `old_session_id`                        |
| `start scheduler`              | Scheduler startup               | —                                       |
| `start watcher`                | Watcher startup                 | —                                       |
| `import page`                  | Reference import (page)         | `topic`                                 |
| `import git`                   | Reference import (git)          | `topic`                                 |
| `import crawl`                 | Reference import (crawl)        | `topic`                                 |
| `import references`            | Reference import (pipeline)     | `topic`                                 |
| `fetch url crawl4ai`           | URL fetch via Crawl4AI          | `url`                                   |
| `fetch url reqwest`            | URL fetch via reqwest           | `url`                                   |
| `search web`                   | Web search                      | `query`                                 |

> **Note:** `Chat error` is a log event (not a span) — query it with
> `WHERE kind = 'log' AND message LIKE '%Chat error%'`. Other error log events follow
> a similar pattern.

## Accessing JSON Attributes

```sql
-- Text value
attributes->>'gen_ai.request.model'

-- Cast to number for aggregation
(attributes->'gen_ai.usage.input_tokens')::int

-- Filter on nested value
WHERE attributes->>'gen_ai.system' = 'openai_oauth'
```

## Common Query Recipes

### Recent errors

```sql
SELECT span_name, message, attributes->>'error' as error,
       start_timestamp
FROM records
WHERE otel_status_code = 'ERROR'
ORDER BY start_timestamp DESC
LIMIT 10
```

### LLM token usage (last 24h)

```sql
SELECT attributes->>'gen_ai.request.model' as model,
       COUNT(*) as calls,
       SUM((attributes->'gen_ai.usage.input_tokens')::int) as input_tok,
       SUM((attributes->'gen_ai.usage.output_tokens')::int) as output_tok,
       AVG(duration) as avg_duration_s
FROM records
WHERE span_name = 'request completion'
  AND attributes->>'gen_ai.usage.input_tokens' IS NOT NULL
GROUP BY model
```

### Full trace tree for a specific trace

```sql
SELECT span_id, parent_span_id, span_name, message,
       start_timestamp, duration
FROM records
WHERE trace_id = '<TRACE_ID>'
ORDER BY start_timestamp
```

### Agent runs with performance

```sql
SELECT attributes->>'gen_ai.agent.name' as agent,
       attributes->>'gen_ai.agent.id' as session,
       duration
FROM records
WHERE span_name = 'execute agent'
ORDER BY start_timestamp DESC
LIMIT 10
```

### Tool execution frequency

```sql
SELECT attributes->>'gen_ai.tool.name' as tool_name,
       COUNT(*) as cnt,
       AVG(duration) as avg_s
FROM records
WHERE span_name = 'run tool'
  AND attributes->>'gen_ai.tool.name' IS NOT NULL
GROUP BY tool_name
ORDER BY cnt DESC
```

### Errors in a specific source file

Use the dedicated tool instead of raw SQL:

```
mcp__logfire__query_find_exceptions_in_file(filepath="src/chat/session.rs", age=1440)
```

## Tool Reference (Quick)

| Tool                            | When to use                                 |
| ------------------------------- | ------------------------------------------- |
| `query_run`                     | Any SQL query — most flexible               |
| `query_find_exceptions_in_file` | Quick error lookup by source file path      |
| `query_schema_reference`        | Get full column definitions if unsure       |
| `project_logfire_link`          | Generate a Logfire UI URL for a trace_id    |
| `token_info`                    | Verify auth / check scopes                  |
| `alert_*`                       | Create/manage alerts (SQL-based conditions) |
| `dashboard_*`                   | Create/manage Perses-format dashboards      |

## `age` Parameter

The `age` parameter is in **minutes**. Common values:

- Last hour: `age: 60`
- Last 24h: `age: 1440`
- Last 7d: `age: 10080`
- Last 30d: `age: 43200` (maximum)

## DataFusion SQL Gotchas

- No `DISTINCT ON` — use window functions or subqueries instead.
- `ORDER BY` columns in `SELECT DISTINCT` must appear in the select list.
- JSON access: `->>` returns text, `->` returns JSON (must cast for math).
- String literals use single quotes only.
- `LIMIT` and `ORDER BY` work as expected.
- Aggregation functions: `COUNT`, `SUM`, `AVG`, `MIN`, `MAX` all work.
- Boolean columns: filter with `= true` / `= false` (not `IS TRUE`).

## Dashboards

No dashboards exist yet. To create one:

1. `dashboard_create` with `name`, `slug`, `project: "ghost"`.
2. `dashboard_add_panel` for each visualization.
3. Queries use `{"kind": "LogfireQuery", "spec": {"query": "..."}}`.
4. Panel types: `TimeSeriesChart`, `BarChart`, `StatChart`, `Table`, `GaugeChart`.
5. Grid layout: `x`/`y`/`width`/`height` (24-column grid).

## Alerts

No alerts exist yet. Use `alert_create` with a SQL query that returns rows when the
alert condition is met. The alert fires when the query returns any rows.
