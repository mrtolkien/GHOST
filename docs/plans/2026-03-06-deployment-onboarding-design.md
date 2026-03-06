# GHOST Deployment & Onboarding — Design

## Goal

A single `curl | sh` command installs and runs the full GHOST stack on a Mac Mini (Apple
Silicon). Only prerequisite: a terminal. Only interactive steps: choose an LLM provider
and paste credentials.

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

## File Layout

```
scripts/install/
  install.sh      # entry point, curled and piped to sh
  onboard.py      # interactive config wizard, curled by install.sh, run with uv
```

## Install Script (install.sh)

Steps, in order:

1. **Install Nix** — detect if present. If not, install via Determinate Systems
   installer (`--no-confirm`). `exec $SHELL` to reload PATH.

2. **Install packages via nix profile** —
   - `llama-cpp` (Metal-enabled on aarch64-darwin)
   - `docling-serve`
   - `docker-client`, `docker-compose`
   - `colima` (only if `docker info` fails, i.e. no Docker Desktop)
   - `uv` (for running the onboarding script)

3. **Start colima** (if installed) — `colima start`.

4. **Download embedding model** — fetch `qwen3-embedding:8b` GGUF into
   `~/.local/share/ghost/models/`.

5. **Register launchd services** — write plist files to `~/Library/LaunchAgents/`:
   - `com.ghost.llama-server` — `llama-server --model <path> --embedding --port 11434`
   - `com.ghost.docling-serve` — `docling-serve --port 5001`
   - Load both with `launchctl load`.

6. **Run onboarding** — download `onboard.py` from GitHub, run with
   `uv run /tmp/ghost-onboard.py`.

7. **Docker compose up** — download `docker-compose.yml` + `searxng-settings.yml` from
   the repo, run `docker compose up -d`.

## Onboarding Script (onboard.py)

Standalone Python script with inline PEP 723 deps (`questionary`, `toml`). Run via
`uv run`. Not part of the Ghost Rust CLI — clean separation.

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
  embeddings pointing to `host.docker.internal:11434`, web service URLs.

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
provider = "openrouter"            # from selection
model = "anthropic/claude-sonnet-4"  # from selection
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

## Docker Compose Changes

The existing `docker-compose.yml` needs two adjustments for the hybrid setup:

- `docling-serve` service removed (runs natively via launchd)
- Ghost's env vars updated: `DOCLING_URL=http://host.docker.internal:5001`, embeddings
  URL uses `host.docker.internal:11434`

This means we need a `docker-compose.install.yml` (or similar) separate from the dev
compose file.

## Separation of Concerns

| Concern                              | Owner                  |
| ------------------------------------ | ---------------------- |
| Install nix, docker, system services | `install.sh`           |
| Interactive config generation        | `onboard.py`           |
| Reading config, running the daemon   | `ghost` (Rust CLI)     |
| Post-install config changes          | `ghost config set ...` |

The onboarding script is an installer artifact. Ghost does not depend on it or know
about it. After install, all config management goes through `ghost config`.

## Not In Scope

- Linux / systemd support (future — same design, swap launchd for systemd)
- Local LLM chat models (only embeddings via llama-cpp for now)
- Tailscale / remote access
- Portainer
- Uninstall script
- Ollama (llama-cpp directly is lighter for single-model use)
