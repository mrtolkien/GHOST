# GHOST Deployment & Onboarding — Design

## Goal

A single `curl | sh` command installs and runs the full GHOST stack on a Mac Mini
(Apple Silicon). Only prerequisite: a terminal. Only interactive steps: choose an LLM
provider and paste credentials.

## Architecture

Two execution domains, split by GPU needs:

```
macOS native (launchd, Metal GPU)
  llama-server :11434  — embeddings (qwen3-embedding:8b)
  docling-serve :5001  — PDF/document processing

Docker (Desktop or Colima)
  ghost         — main daemon, connects to host services
  crawl4ai      — web extraction
  searxng       — web search
```

Ghost's container reaches host services via `host.docker.internal`.

## Repository Layout

```
deploy/
  common/
    onboard.py                   # interactive config wizard (all platforms)
    searxng-settings.yml
    Dockerfile                   # ghost image build
  macos/
    install.sh                   # entry point, curled and piped to sh
    docker-compose.yml           # self-contained: ghost + crawl4ai + searxng
    com.ghost.llama-server.plist # launchd service template
    com.ghost.docling-serve.plist
  linux/                         # future: systemd units, install script, compose
```

Existing root-level `docker-compose.yml` and `docker-compose.local.yml` stay as-is
for development use. `docker/Dockerfile` moves to `deploy/common/Dockerfile`.

## Install Script (deploy/macos/install.sh)

Steps, in order:

1. **Install Nix** — detect if present. If not, install via Determinate Systems
   installer (`--no-confirm`). Source nix profile to pick up PATH.

2. **Install packages via nix profile** —
   - `llama-cpp` (Metal-enabled on aarch64-darwin)
   - `docling-serve`
   - `docker-client`, `docker-compose`
   - `colima` (only if `docker info` fails, i.e. no Docker Desktop)
   - `uv` (for running the onboarding script)

3. **Start colima** (if installed) — `colima start`.

4. **Download embedding model** — fetch `qwen3-embedding:8b` GGUF into
   `~/.local/share/ghost/models/`.

5. **Register launchd services** — install plist files to `~/Library/LaunchAgents/`,
   load with `launchctl load`:
   - `com.ghost.llama-server` — `llama-server --model <path> --embedding --port 11434`
   - `com.ghost.docling-serve` — `docling-serve --port 5001`

6. **Run onboarding** — curl `onboard.py` from GitHub to `/tmp/`, run with
   `uv run /tmp/ghost-onboard.py`.

7. **Docker compose up** — curl `docker-compose.yml` + `searxng-settings.yml` to
   `~/.config/ghost/`, run `docker compose up -d`.

## Onboarding Script (deploy/common/onboard.py)

Standalone Python script with inline PEP 723 deps (`questionary`, `toml`). Run via
`uv run`. Not part of the Ghost Rust CLI — clean separation. Shared across platforms
(the install shell script is platform-specific, the onboarding wizard is not).

Interactive prompts:

1. **Select LLM provider** — arrow-key list: OpenRouter, Kimi, OpenAI OAuth.
2. **Enter API key** — text input, validated non-empty.
3. **Select default model** — provider-specific list of popular models with context
   window sizes pre-filled.
4. **Enter Discord bot token** — text input.
5. **Enter Discord user ID** — text input.

Outputs:

- `~/.config/ghost/.env` — secrets (API key, Discord token)
- `~/.config/ghost/config.toml` — provider config, model aliases, Discord user ID,
  embeddings pointing to localhost, web service URLs.

## Docker Compose (deploy/macos/docker-compose.yml)

Self-contained file, no `-f` merging. Curled to `~/.config/ghost/docker-compose.yml`
so the user can inspect and edit it later.

Services: ghost, crawl4ai, searxng. No docling (runs natively). Ghost env vars point
to host services via `host.docker.internal`.

## Generated Config

### ~/.config/ghost/.env

```
OPENROUTER_API_KEY=sk-or-...     # or KIMI_API_KEY, etc.
DISCORD_TOKEN=...
```

### ~/.config/ghost/config.toml

```toml
[discord]
allowed_user_id = "..."

[models.primary]
provider = "openrouter"
model = "anthropic/claude-sonnet-4"
context_window = 200000

[embeddings]
url = "http://host.docker.internal:11434"
model = "qwen3-embedding:8b"

[web]
crawl4ai_url = "http://crawl4ai:11235"
docling_url = "http://host.docker.internal:5001"

[web.search]
provider = "searxng"
url = "http://searxng:8080"
```

## Separation of Concerns

| Concern | Owner |
|---|---|
| Install nix, docker, system services | `install.sh` (platform-specific) |
| Interactive config generation | `onboard.py` (shared) |
| Reading config, running the daemon | `ghost` (Rust CLI) |
| Post-install config changes | `ghost config set ...` |

The onboarding script is an installer artifact. Ghost does not depend on it or know
about it. After install, all config management goes through `ghost config`.

## Not In Scope

- Linux / systemd support (future — same design, swap launchd for systemd)
- Local LLM chat models (only embeddings via llama-cpp for now)
- Tailscale / remote access
- Portainer
- Uninstall script
- Ollama (llama-cpp directly is lighter for single-model use)
