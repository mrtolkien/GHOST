---
title: External Dependencies
description:
  Why GHOST depends on external services and what each one does.
---

GHOST relies on a few external services. This page explains *why* each one exists and what
it provides.

:::note
All dependencies beyond the binary itself are managed by `ghost init` via nix and
podman/docker compose. See [Services](/getting-started/services/) for how to manage them
after setup.
:::

### Container runtime (podman or docker)

Several services (SearXNG, Crawl4AI, Chrome, Docling) run as containers. GHOST supports
both podman and docker — whichever is available on your system.

**On macOS**, `ghost init` can install podman via nix automatically. After a one-time
Rosetta 2 install on Apple Silicon, it works out of the box.

**On Linux**, installing Docker via your distribution's package manager is currently the
simplest path. Podman via nix works but requires extra system-level setup (`newuidmap`
with setuid privileges, `/etc/subuid` and `/etc/subgid` entries, container config files)
that nix cannot manage on its own. If you already have Docker installed, GHOST will
detect and use it — no extra steps needed.

## llama-server (llama.cpp)

**What**: Local LLM inference server (llama.cpp)
**Why**: GHOST uses llama-server to generate embeddings for semantic search. The
`qwen3-embedding:8b` model converts knowledge entries into vectors stored in sqlite-vec,
enabling similarity-based search alongside BM25 keyword matching.

**Used by**: Knowledge indexing, hybrid search

Installed via nix and managed as a systemd/launchd service by `ghost init`.

```sh
systemctl --user status llama-server
```

## Crawl4AI

**What**: Headless browser service for web page extraction
**Why**: Many web pages require JavaScript rendering or have complex layouts that simple
HTTP fetching can't handle. Crawl4AI runs a headless browser that extracts clean,
readable content from any page — even SPAs and dynamically-loaded content.

**Used by**: `web_fetch` tool, deep-research agent

```sh
podman compose -f ~/GHOST/services/docker-compose.yml ps crawl4ai
```

## Chrome (headless-shell)

**What**: Headless Chrome browser
**Why**: Crawl4AI uses Chrome's DevTools Protocol (CDP) to render JavaScript-heavy pages.
The `chromedp/headless-shell` image provides a lightweight, CDP-compatible Chrome
instance.

**Used by**: Crawl4AI

- **Port**: 9222 (CDP)

## SearXNG

**What**: Self-hosted metasearch engine
**Why**: Privacy-respecting web search without API key dependencies. SearXNG aggregates
results from multiple search engines (Google, Bing, DuckDuckGo, etc.) without tracking.
It replaces the Brave Search API as the primary search backend.

**Used by**: `web_search` tool

```sh
podman compose -f ~/GHOST/services/docker-compose.yml ps searxng
```

## SigNoz (optional)

**What**: Self-hosted observability platform (traces, metrics, logs)
**Why**: GHOST exports OpenTelemetry traces for every LLM call, tool execution, and
agent run. SigNoz provides a web UI to explore traces, debug latency, and monitor
token usage — all self-hosted with no cloud dependency.

**Used by**: Observability pipeline (when `OTEL_EXPORTER_OTLP_ENDPOINT` is set)

SigNoz runs as a separate Docker Compose stack. Ask your GHOST about the **services**
skill's observability extra for setup instructions.

```sh
podman compose -f docker-compose.signoz.yml up -d
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
