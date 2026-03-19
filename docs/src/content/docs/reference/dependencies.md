---
title: External Dependencies
description:
  Why GHOST depends on external services and what each one does.
---

GHOST relies on a few external services. This page explains *why* each one exists and what
it provides.

:::note
In the future, all dependencies beyond the binary itself will be managed through Nix. For
now, you need to set them up manually — see [Installation](/getting-started/installation/)
for setup steps.
:::

## Ollama

**What**: Local LLM inference server
**Why**: GHOST uses Ollama to generate embeddings for semantic search. The
`qwen3-embedding:8b` model converts knowledge entries into vectors stored in sqlite-vec,
enabling similarity-based search alongside BM25 keyword matching.

**Used by**: Knowledge indexing, hybrid search

```bash
ollama pull qwen3-embedding:8b
```

## Crawl4AI

**What**: Headless browser for web page extraction
**Why**: Many web pages require JavaScript rendering or have complex layouts that simple
HTTP fetching can't handle. Crawl4AI runs a headless browser that extracts clean,
readable content from any page — even SPAs and dynamically-loaded content.

**Used by**: `web_fetch` tool, deep-research agent

```bash
docker run -d -p 11235:11235 unclecode/crawl4ai
```

## SearXNG

**What**: Self-hosted metasearch engine
**Why**: Privacy-respecting web search without API key dependencies. SearXNG aggregates
results from multiple search engines (Google, Bing, DuckDuckGo, etc.) without tracking.
It replaces the Brave Search API as the primary search backend.

**Used by**: `web_search` tool

## SigNoz (optional)

**What**: Self-hosted observability platform (traces, metrics, logs)
**Why**: GHOST exports OpenTelemetry traces for every LLM call, tool execution, and
agent run. SigNoz provides a web UI to explore traces, debug latency, and monitor
token usage — all self-hosted with no cloud dependency.

**Used by**: Observability pipeline (when `OTEL_EXPORTER_OTLP_ENDPOINT` is set)

SigNoz runs as a separate Docker Compose stack:

```bash
docker compose -f docker-compose.signoz.yml up -d
# UI at http://localhost:3301
```

:::tip
SigNoz is optional. Without it, GHOST still logs to the console. You can also point
`OTEL_EXPORTER_OTLP_ENDPOINT` at any OTLP-compatible backend (Logfire, Datadog,
Grafana Cloud, etc.).
:::

## SQLite + sqlite-vec + FTS5

**What**: Embedded database with vector and full-text extensions
**Why**: GHOST stores everything in a single SQLite database — sessions, messages,
knowledge entries, embeddings. sqlite-vec provides KNN vector search for semantic
similarity. FTS5 provides full-text search with Porter stemming. No external database
server needed.

**Used by**: Everything — this is the core data layer
