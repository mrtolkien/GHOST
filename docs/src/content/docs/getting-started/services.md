---
title: Services
description:
  How GHOST's service stack works — native services, containers, and
  how to manage them.
---

Your GHOST relies on several services to function. The onboarding wizard
(`ghost init`) sets them up, but this page explains how they work and how
to manage them afterward.

## Architecture

Services come in two flavors:

### Native Services (nix + systemd/launchd)

Installed via `nix profile install` and managed as system services.

| Service | Binary | Purpose |
| --- | --- | --- |
| **ghost-daemon** | `ghost` | The GHOST itself |
| **llama-server** | `llama-server` | Embedding generation (llama.cpp) |
| **docling-serve** | `docling-serve` | PDF/document processing |

On Linux, these run as systemd user services:

```sh
systemctl --user status ghost-daemon llama-server docling-serve
systemctl --user restart llama-server
```

On macOS, they run as launchd agents:

```sh
launchctl list | grep com.ghost
```

### Container Services (podman/docker)

Managed via a single Docker Compose file at
`<workspace>/services/docker-compose.yml`.

| Service | Image | Purpose |
| --- | --- | --- |
| **SearXNG** | `searxng/searxng` | Web search (meta search engine) |
| **Crawl4AI** | `unclecode/crawl4ai` | Web page extraction |
| **Chrome** | `chromedp/headless-shell` | Headless browser for Crawl4AI |

Common operations:

```sh
# Status
podman compose -f ~/GHOST/services/docker-compose.yml ps

# Restart all
podman compose -f ~/GHOST/services/docker-compose.yml restart

# View logs
podman compose -f ~/GHOST/services/docker-compose.yml logs -f searxng

# Stop everything
podman compose -f ~/GHOST/services/docker-compose.yml down
```

## File Layout

```
~/.config/ghost/
├── config.toml              # Configuration
└── .env                     # Secrets (API keys, tokens)

~/GHOST/services/
├── docker-compose.yml       # Container stack
└── searxng-settings.yml     # SearXNG configuration

~/.config/systemd/user/      # Linux
├── ghost-daemon.service
├── llama-server.service
└── docling-serve.service
```

## Service Details

### Embeddings (llama-server)

Converts text into numerical vectors for semantic search. Your GHOST
uses these vectors to find relevant notes and references even when exact
words don't match.

- **Model**: `qwen3-embedding:8b` (configurable in `config.toml`)
- **Port**: 11434
- **Config section**: `[embeddings]`

### Web Search (SearXNG)

Self-hosted meta search engine. Aggregates results from Google, Bing,
DuckDuckGo, and others — no API keys needed.

- **Port**: 8080
- **Config section**: `[web.search]`
- **Settings**: `<workspace>/services/searxng-settings.yml`

### Web Fetch (Crawl4AI + Chrome)

Reads web pages and converts them to clean markdown. Crawl4AI renders
JavaScript-heavy pages using a headless Chrome instance.

- **Crawl4AI port**: 11235
- **Chrome port**: 9222 (CDP)
- **Config section**: `[web]` (`crawl4ai_url`, `[[web.browsers]]`)

### Document Processing (Docling)

Converts PDFs, Word documents, and presentations to markdown. Handles
OCR, table extraction, and complex layouts.

- **Port**: 5001
- **Config section**: `[docling]`

## Optional: Observability (SigNoz)

SigNoz gives you distributed tracing, metrics, and logs for your GHOST
via OpenTelemetry. It's not set up by the wizard, but your GHOST knows
how to help — ask it about the **services** skill's observability extra.

Quick setup:

1. Ask your GHOST to read the services skill's observability extra
2. It will guide you through deploying the SigNoz stack and configuring
   `OTEL_EXPORTER_OTLP_ENDPOINT`

## Optional: Tailscale

Tailscale provides secure remote access to your GHOST without opening
ports. Your GHOST can help — ask it about the **services** skill's
tailscale extra.

## Troubleshooting

### A service won't start

Check its logs:

```sh
# Native service
journalctl --user -u llama-server -f

# Container service
podman compose -f ~/GHOST/services/docker-compose.yml logs crawl4ai
```

### Reconfigure everything

```sh
ghost init
```

This re-runs the wizard with your existing values pre-filled.

### Nix garbage collection

Nix stores grow over time. Clean up old generations periodically:

```sh
nix-collect-garbage -d
```
