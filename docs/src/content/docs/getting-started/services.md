---
title: Services
description:
  How GHOST's service stack works — native services, containers, GPU acceleration, and
  how to manage them.
---

Your GHOST relies on several services to function. The onboarding wizard (`ghost init`)
sets them up, but this page explains how they work and how to manage them afterward.

## Why Native vs Container?

| Service           | Runs as      | GPU?     | Why                                                              |
| ----------------- | ------------ | -------- | ---------------------------------------------------------------- |
| **GHOST**         | Native (nix) | No       | Simpler for self updating + containers management                |
| **llama-server**  | Native (nix) | **Yes**  | Embedding inference — 10-50x faster with GPU (Metal, CUDA, ROCm) |
| **docling-serve** | Native (nix) | Optional | OCR benefits from GPU but works on CPU                           |
| **SearXNG**       | Container    | No       | Lightweight HTTP proxy, no compute                               |
| **Crawl4AI**      | Container    | No       | Browser automation, CPU-bound                                    |
| **Chrome**        | Container    | No       | Headless rendering for Crawl4AI                                  |

**Why llama-server must run natively**: Embedding generation is the only service that
truly needs GPU acceleration. On macOS, containers cannot access Metal GPUs —
llama-server must run on the host to use Apple Silicon. On Linux, GPU passthrough to
containers is possible but adds complexity; the nix-native path auto-detects CUDA and
ROCm without extra configuration.

On CPU-only systems (small VPS), llama-server still works — embedding models are small
enough for CPU inference, just slower. If performance is unacceptable, point
`[embeddings].url` at a remote instance instead.

## Native Services (nix + systemd/launchd)

Installed via `nix profile install` and managed as system services.

| Service           | Binary          | Default port |
| ----------------- | --------------- | ------------ |
| **ghost-daemon**  | `ghost`         | —            |
| **llama-server**  | `llama-server`  | 11434        |
| **docling-serve** | `docling-serve` | 5001         |

On Linux (systemd):

```sh
systemctl --user status ghost-daemon llama-server docling-serve
systemctl --user restart llama-server
journalctl --user -u llama-server -f
```

On macOS (launchd):

```sh
launchctl list | grep com.ghost
launchctl kickstart -k gui/$(id -u)/com.ghost.llama-server
```

## Container Services (podman/docker)

Managed via a single compose file at `<workspace>/services/docker-compose.yml`.

| Service      | Image                     | Default port |
| ------------ | ------------------------- | ------------ |
| **SearXNG**  | `searxng/searxng`         | 8080         |
| **Crawl4AI** | `unclecode/crawl4ai`      | 11235        |
| **Chrome**   | `chromedp/headless-shell` | 9222         |

```sh
# Status
podman compose -f ~/GHOST/services/docker-compose.yml ps

# Restart
podman compose -f ~/GHOST/services/docker-compose.yml restart

# Logs
podman compose -f ~/GHOST/services/docker-compose.yml logs -f searxng

# Stop
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

Converts text into numerical vectors for semantic search. Your GHOST uses these to find
relevant notes and references even when exact words don't match. This is the only
service that requires GPU for good performance.

- **Model**: `qwen3-embedding:8b` (configurable in `config.toml`)
- **Config section**: `[embeddings]`

### Web Search (SearXNG)

Self-hosted meta search engine. Aggregates results from Google, Bing, DuckDuckGo, and
others — no API keys needed.

- **Config section**: `[web.search]`
- **Settings**: `<workspace>/services/searxng-settings.yml`

### Web Fetch (Crawl4AI + Chrome)

Reads web pages and converts them to clean markdown. Crawl4AI renders JavaScript-heavy
pages using a headless Chrome instance. The headless Chrome instance is also usable as a
browser directly by the GHOST.

- **Config section**: `[web]` (`crawl4ai_url`, `[[web.browsers]]`)

### Document Processing (Docling)

Converts PDFs, Word documents, and presentations to markdown. Handles OCR, table
extraction, and complex layouts.

- **Config section**: `[docling]`

## Optional: Observability (SigNoz)

SigNoz gives you distributed tracing, metrics, and logs for your GHOST via
OpenTelemetry. It's not set up by the wizard, but your GHOST knows how to help — ask it
about the **services** skill's observability extra.

## Optional: Tailscale

Tailscale provides secure remote access to your GHOST without opening ports. Ask your
GHOST about the **services** skill's tailscale extra.

## Troubleshooting

### Reconfigure everything

```sh
ghost init
```

Re-runs the wizard with your existing values pre-filled.
