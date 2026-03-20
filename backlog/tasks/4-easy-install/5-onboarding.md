We need a good onboarding flow:

- Check nix install (or install nix ourself?)
- Setup model provider
  - The model could be used for some questions during the onboarding if there's a need
    to debug things?
- Setup embeddings
- Setup discord (bot token + approved user id)
- Setup tailscale (both host and clients)
- Setup opentelemetry -> Optional

---

Onboarding wizard + external service management. Deferred until core install/update flow
is solid.

## Onboarding (`ghost init` interactive setup)

- LLM provider selection (OpenRouter, Kimi, OpenAI OAuth) + API key
- Discord token + user ID
- For each service (embeddings, search, crawl, docling): ask "local Docker / remote URL
  / skip?"
- Generate `config.toml` + `.env` + `docker-compose.yml` from answers
- Replace `deploy/common/onboard.py` with native Rust implementation

## Service management

- Ghost should manage its own sidecar services (start/stop/restart containers)
- Either a skill that teaches Ghost to run compose commands, or a `ghost stack` CLI
- Health checks: Ghost detects when a service goes down, notifies operator
- See also: `deployment_per_platform.md` for per-platform service fallback chains
  (Firecrawl, Brave API, remote embeddings, etc.)

---

- Onboarding should include oauth sync
- Onboarding/cli config picker should properly list available models for all providers
  - For example, get top models on openrouter, ...
  - Check model-picker spec
- Onboarding/deployment should work on Linux with all GPU types (Nvidia, AMD, Intel,
  ...)

---

Create a clean services list and docker compose file to be included in the binary and
deploy it with podman rootless by default (or docker if available):

- crawl4ai
- searxng (also possible native with nix, but no gain?)
- Headless chrome w/ CDP

Native would be better for:

- Docling (maybe even use the CLI? Can we install it with nix as part of the flake?)
- Llama.cpp

---

# Design Spec: `ghost init` Onboarding

## Overview

`ghost init` becomes the single entry point for all first-run setup and reconfiguration.
It replaces `deploy/common/onboard.py` (Python wizard) with a native Rust implementation
that handles: LLM provider setup, Discord configuration, service installation (native +
containers), workspace bootstrapping, daemon service file generation, and first-run
health checks.

### Goals

- Anyone can install and run GHOST with the fewest possible commands
- Clear, beautiful terminal UX with context at every step (what each service is, why
  it's needed, links to learn more)
- Fully scriptable for automated deployment and testing
- Nix as the only prerequisite — everything else is installed or configured by the
  wizard

### Non-goals

- Installing Nix itself (show the Determinate installer URL and exit)
- Managing Tailscale or observability (documented in the services skill, not in the
  wizard)
- Live-fetching model catalogs from providers (tested and found unreliable — most
  providers don't have clean listing APIs; link to web pages instead)
- GPU detection and optimization (llama.cpp and docling auto-detect available GPU
  backends at runtime; the nix packages include Metal/CUDA/ROCm support where available)

## Architecture

### New code

- **`src/cli/init.rs`** — Rewritten. Orchestrates the wizard phases, delegates to
  `src/onboarding/`.
- **`src/onboarding/`** — New module:
  - `detect.rs` — Environment detection (nix, platform, container runtime, running
    services, existing config, nix packages)
  - `wizard.rs` — Interactive wizard flow (phases 1–5), prompt definitions
  - `provider.rs` — Provider selection, API key validation (real LLM call)
  - `discord.rs` — Discord setup guidance and prompts
  - `services.rs` — Per-service prompts, nix profile installation, compose file
    generation
  - `config_writer.rs` — Config diff, config.toml + .env generation
  - `service_files.rs` — systemd/launchd unit generation for daemon + native services
  - `health.rs` — Post-install health probes for all services
  - `agent.rs` — On-demand onboarding assistant (LLM chat in terminal)
- **`assets/skills/services/skill.md`** — Bundled skill: service management
- **`assets/skills/services/observability.md`** — Extra: SigNoz OTEL stack
- **`assets/skills/services/tailscale.md`** — Extra: Tailscale remote access
- **`assets/onboarding-agent-prompt.md`** — System prompt for the onboarding assistant

### Dependencies (require discussion per CLAUDE.md rules)

- **`cliclack`** — Wizard framing: intro/outro, styled prompts, spinners, note boxes.
  Pure Rust, inspired by @clack/prompts. Provides the polished session feel.
- **`dialoguer`** — FuzzySelect for model/provider pickers if cliclack's Select is
  insufficient. Same author as `console`/`indicatif`.

These are onboarding-only dependencies (not used at runtime by the daemon). Both are
pure Rust with no C dependencies.

### Scriptability

Every interactive prompt has a corresponding CLI flag. When all required flags are
provided, the wizard runs non-interactively (no prompts, no spinners — just status
lines). Partial flags fill in what they can and prompt for the rest.

```
# Fully non-interactive
ghost init \
  --provider openrouter \
  --api-key "$KEY" \
  --model "anthropic/claude-sonnet-4" \
  --context-window 200000 \
  --discord-token "$TOKEN" \
  --discord-user "$UID" \
  --embeddings local \
  --search local \
  --crawl local \
  --docling local \
  --start

# Interactive (default)
ghost init
```

### Existing config handling

If `config.toml` already exists when `ghost init` runs:

1. Detect and load the existing config
2. Offer: "Update existing config / Fresh install / Cancel"
3. Pre-fill prompts with existing values (user can accept with Enter or change)
4. At write time (Phase 4), show a diff of all changes before applying

## Wizard Flow

### Phase 0 — Detection (~1 second, no prompts)

Runs automatically before any interactive step. Probes the environment and stores
results in a `DetectedEnvironment` struct used by all subsequent phases.

```
╭─────────────────────────────────────╮
│  GHOST — First-time setup           │
╰─────────────────────────────────────╯

  Checking environment...
  ✓ Nix installed
  ✓ Platform: Linux (systemd)
  ✓ Podman available
  ✗ llama-server not found
  ✗ docling-serve not found
  ● Existing config.toml detected
```

**Checks performed:**

| Check                         | Method                                        | Hard fail?                                  |
| ----------------------------- | --------------------------------------------- | ------------------------------------------- |
| Nix installed                 | `which nix`                                   | Yes — print Determinate installer URL, exit |
| Platform                      | `cfg!(target_os)` + check for systemd/launchd | No (informational)                          |
| Container runtime             | `which podman` then `which docker`            | No (skip container services if neither)     |
| llama-server in PATH          | `which llama-server`                          | No                                          |
| docling-serve in PATH         | `which docling-serve`                         | No                                          |
| Ollama/llama-server on :11434 | HTTP probe                                    | No                                          |
| SearXNG on :8080              | HTTP probe                                    | No                                          |
| Chrome on :9222               | HTTP probe `/json/version`                    | No                                          |
| Docling on :5001              | HTTP probe                                    | No                                          |
| Crawl4AI                      | HTTP probe                                    | No                                          |
| Existing config.toml          | File existence check                          | No                                          |
| Existing .env                 | File existence check                          | No                                          |

### Phase 1 — LLM Provider (required)

```
◇ Select your LLM provider

  OpenRouter gives you access to hundreds of models from all providers
  through a single API key. It's the recommended choice for most setups.

  ● OpenRouter (recommended — access to all models)
  ○ Anthropic
  ○ Kimi
  ○ ChatGPT OAuth (uses Claude Code credentials)

◇ Enter your OpenRouter API key
  ●●●●●●●●●●●●●●●●●●●●

╭───────────────────────────────────────────────────────────╮
│  Browse available models at:                              │
│  https://openrouter.ai/rankings                           │
│                                                           │
│  Copy the model ID (e.g. "anthropic/claude-sonnet-4")     │
╰───────────────────────────────────────────────────────────╯

◇ Enter model ID
  anthropic/claude-sonnet-4

◇ Context window size (tokens)
  200000

  Validating provider connection...
  ✓ Provider verified — model responded successfully

  ℹ Press [h] at any prompt for AI-assisted help
```

**Per-provider details:**

| Provider      | Auth method                    | Catalog URL                                            | Notes                                               |
| ------------- | ------------------------------ | ------------------------------------------------------ | --------------------------------------------------- |
| OpenRouter    | API key (`OPENROUTER_API_KEY`) | https://openrouter.ai/rankings                         | Recommended default                                 |
| Anthropic     | Claude platform OAuth          | https://docs.anthropic.com/en/docs/about-claude/models | Reads `~/.claude/.credentials.json` (claudeAiOauth) |
| Kimi          | API key (`KIMI_API_KEY`)       | https://kimi.com                                       |                                                     |
| ChatGPT OAuth | OpenAI device OAuth            | https://developers.openai.com/codex/models             | Uses `OpenAiOAuthClient` token store                |

**OAuth providers have separate credential flows:**

- **Anthropic**: Reads from `~/.claude/.credentials.json` (the `claudeAiOauth` section).
  These credentials come from Claude Code / Claude platform — they are NOT the same as a
  direct API key. If the credentials file doesn't exist, show a note box: "Anthropic
  OAuth requires Claude Code credentials. Please run `claude` first to authenticate,
  then re-run `ghost init`." On headless servers where browser-based OAuth is
  impossible, instruct the user to authenticate on a local machine and copy
  `~/.claude/.credentials.json` to the server.
- **ChatGPT OAuth**: Uses `OpenAiOAuthClient` from `src/auth/openai_oauth.rs` which
  initiates a device-code OAuth flow. The wizard runs `ghost auth codex` inline — this
  prints a URL + code, the user authenticates in a browser, and tokens are stored
  locally. Works on headless servers via the device-code flow (user opens URL on any
  device).

**Validation call**: A real completion request — e.g. system="Reply with OK",
user="ping". Confirms the credentials, model ID, and endpoint all work. On failure: show
the error, offer to retry or go back.

After validation succeeds, the onboarding agent becomes available (see "Onboarding
Agent" section below).

### Phase 2 — Discord

```
╭───────────────────────────────────────────────────────────╮
│  Discord Bot Setup                                        │
│                                                           │
│  Your GHOST communicates with you through a Discord bot.  │
│  You'll need to create one in the Discord Developer       │
│  Portal and invite it to your server.                     │
│                                                           │
│  1. Go to https://discord.com/developers/applications     │
│  2. Click "New Application" → name it (e.g. "GHOST")      │
│  3. Go to "Bot" tab:                                      │
│     → Click "Reset Token" → copy the token                │
│     → Enable "Message Content Intent" under               │
│       Privileged Gateway Intents                          │
│  4. Go to "OAuth2" → "URL Generator":                     │
│     → Check "bot" scope                                   │
│     → Check permissions: Send Messages,                   │
│       Read Message History, Attach Files,                 │
│       Use Slash Commands, Embed Links                     │
│  5. Copy the generated URL → open it → invite the bot     │
│     to your server                                        │
╰───────────────────────────────────────────────────────────╯

◇ Paste your bot token
  ●●●●●●●●●●●●●●●●●●●●

◇ Your Discord user ID
  (Enable Developer Mode in Settings → Advanced,
   then right-click your name → Copy User ID)
  123456789012345678
```

**Validation**: User ID validated as numeric (17-18 digits). Bot token validated with a
real Discord API call (GET `/api/v10/users/@me` with `Authorization: Bot <token>`) —
this catches typos immediately rather than failing at daemon start.

### Phase 3 — Services

Each service gets: a description of what it is and why the GHOST needs it, detected
state, and a picker. The picker options depend on what was detected in Phase 0.

#### Embeddings (llama-server)

```
── Embeddings (llama-server) ──────────────────────────────

  Your GHOST converts text into numerical vectors for semantic search.
  This lets it find relevant notes, references, and past conversations
  even when the exact words don't match. Powered by llama.cpp.

  https://github.com/ggml-org/llama.cpp

  ✗ llama-server not found in PATH
  (or: ✓ llama-server running on :11434)

◇ How should embeddings be set up?
  ● Install llama-server via nix (recommended)
  ○ Use existing llama-server / Ollama (detected on :11434)  ← only if detected
  ○ Remote — enter URL
  ○ Skip (embeddings will be unavailable)

◇ Embedding model
  qwen3-embedding:8b

  Installing llama-server...
  ✓ llama-server installed via nix profile
```

If "Install via nix" is selected: run `nix profile install nixpkgs#llama-cpp` with a
spinner. The embedding model file download happens at daemon startup, not during init.

**Smart defaults by environment**: On low-memory systems (< 4GB RAM detected), the
wizard should default to "Remote" or "Skip" for llama-server and docling rather than
"Install via nix", and show a note explaining why. Detection is lightweight:
`sysinfo::System::total_memory()` or reading `/proc/meminfo`.

#### Web Search (SearXNG)

```
── Web Search (SearXNG) ───────────────────────────────────

  Your GHOST searches the web to find up-to-date information,
  answer questions, and research topics. SearXNG is a self-hosted
  meta search engine that aggregates results from Google, Bing,
  DuckDuckGo, and others — no API keys needed.

  https://docs.searxng.org

  ✓ Podman available

◇ How should web search be set up?
  ● Local with podman (recommended — lightweight, ~50MB RAM)
  ○ Brave Search API (requires API key, pay per query)
  ○ Remote SearXNG — enter URL
  ○ Skip (web search will be unavailable)
```

If "Brave Search API" selected: prompt for `BRAVE_API_KEY`.

#### Web Fetch (Crawl4AI + Headless Chrome)

```
── Web Fetch (Crawl4AI + Chrome) ──────────────────────────

  Your GHOST reads web pages to extract content from URLs — documentation,
  articles, search results. Crawl4AI renders JavaScript-heavy pages and
  converts HTML to clean markdown. It uses a headless Chrome instance for
  rendering.

  https://github.com/unclecode/crawl4ai

  ✓ Podman available

◇ How should web fetching be set up?
  ● Local with podman (recommended)
  ○ Remote — enter Crawl4AI and Chrome URLs
  ○ Skip (web fetch will fall back to basic HTML extraction)
```

#### Document Processing (Docling)

```
── Document Processing (Docling) ──────────────────────────

  Your GHOST processes PDFs, Word documents, and presentations,
  converting them to markdown for indexing and reference. Docling
  handles OCR, table extraction, and complex document layouts.

  https://github.com/docling-project/docling

  ✗ docling-serve not found in PATH

◇ How should document processing be set up?
  ● Install natively via nix (recommended — better performance)
  ○ Local with podman
  ○ Remote — enter URL
  ○ Skip (document import will be unavailable)
```

If "Install via nix" selected: `nix profile install nixpkgs#docling-serve` with spinner.

#### Compose file generation

All services selecting "Local with podman/docker" are assembled into a single
`docker-compose.yml`. The compose file is generated from a template with only the
selected services included. The SearXNG settings file (`searxng-settings.yml`) is also
written if SearXNG is selected.

Both templates are `include_str!`'d into the binary from `assets/`.

**Container runtime**: Podman is preferred and runs rootless by default (no daemon, no
root). The wizard detects `podman` first, falls back to `docker`. All compose commands
use whichever runtime was detected. No special rootless configuration is needed —
podman's default mode is rootless.

**Networking strategy**: The generated compose file uses `network_mode: host` for all
containers on Linux (simplest — containers share the host network, all services reach
each other on localhost). On macOS (where host networking is unavailable in Docker), use
a bridge network with Docker DNS and `host.docker.internal` for reaching host-side
services (llama-server, docling-serve). The compose template is platform-aware.

**Docling in compose**: If the user selects "Local with podman" for Docling (instead of
native nix add), Docling is included in the compose file as a container service. Both
paths are supported — nix-native is recommended for better performance, but the
container option exists for users who prefer a simpler setup or whose platform has nix
packaging issues for docling.

### Phase 4 — Write Configuration + Install Services

```
  Writing configuration...

── Changes to config.toml ─────────────────────────────────

  + [discord]
  + allowed_user_id = "123456789012345678"
  +
  + [models]
  + default = "primary"
  +
  + [models.primary]
  + provider = "openrouter"
  + model = "anthropic/claude-sonnet-4"
  + context_window = 200000
  +
  + [embeddings]
  + url = "http://127.0.0.1:11434"
  + model = "qwen3-embedding:8b"
  +
  + [web.search]
  + provider = "searxng"
  + url = "http://127.0.0.1:8080"
  +
  + [web]
  + crawl4ai_url = "http://127.0.0.1:11235"
  +
  + [[web.browsers]]
  + name = "chrome"
  + cdp_url = "http://127.0.0.1:9222"
  +
  + [docling]
  + url = "http://127.0.0.1:5001"

◇ Apply these changes?
  Yes

  ✓ ~/.config/ghost/config.toml written
  ✓ ~/.config/ghost/.env written
  ✓ ~/GHOST/ workspace bootstrapped
  ✓ ~/GHOST/services/docker-compose.yml written
  ✓ ~/.config/systemd/user/ghost-daemon.service installed
  ✓ ~/.config/systemd/user/llama-server.service installed
  ✓ ~/.config/systemd/user/docling-serve.service installed
  Starting services...
  ✓ llama-server started
  ✓ docling-serve started
  ✓ Container stack started (podman compose up)
```

**Files written:**

| File                                           | Content                             |
| ---------------------------------------------- | ----------------------------------- |
| `~/.config/ghost/config.toml`                  | Full config from wizard answers     |
| `~/.config/ghost/.env`                         | API keys, tokens (secrets only)     |
| `<workspace>/services/docker-compose.yml`      | Container stack (selected services) |
| `<workspace>/services/searxng-settings.yml`    | SearXNG config (if selected)        |
| `~/.config/systemd/user/ghost-daemon.service`  | Daemon unit (or launchd plist)      |
| `~/.config/systemd/user/llama-server.service`  | Embeddings unit (if nix-installed)  |
| `~/.config/systemd/user/docling-serve.service` | Docling unit (if nix-installed)     |

For existing configs: the diff shows only changed/added lines. Unchanged sections are
not shown.

**Platform-specific actions in Phase 4:**

- **Linux**: Run `loginctl enable-linger $(whoami)` to prevent systemd from killing user
  services on logout. This is already implemented in `src/cli/init.rs` — preserve and
  call it. All systemd units include `TimeoutStopSec=120` to give the daemon time to
  shut down gracefully (known issue: Discord/serenity and SQLite workers can block
  shutdown past the default 90s timeout).
- **macOS**: Write launchd plists with `KeepAlive=true` and `RunAtLoad=true`. Log output
  to `~/Library/Application Support/ghost/logs/`.

**Workspace `services/` directory**: Add `services/` to the directories created by
`config_workspace::bootstrap_workspace_dirs`. This is where the compose file and
service-specific config files (searxng-settings.yml) live.

**`--start` flag semantics**: When `--start` is passed (or the user answers "Yes" to the
interactive prompt), Phase 5 starts **all** services — both native (systemd/launchd
units) and container (podman/docker compose up). It also `enable`s the units so they
start on boot. Without `--start`, services are installed but not started — the user can
start them manually later.

### Phase 5 — Health Checks + Launch

```
── Service Health ─────────────────────────────────────────

  Checking services...
  ✓ LLM provider        openrouter (anthropic/claude-sonnet-4)
  ✓ Embeddings           llama-server on :11434
  ✓ Web search           SearXNG on :8080
  ✓ Web fetch            Crawl4AI + Chrome
  ✓ Document processing  Docling on :5001

◇ Start the ghost daemon now?
  Yes

  ✓ ghost-daemon started
  ✓ First message sent to Discord — check your server!

╭───────────────────────────────────────────────────────────╮
│  Setup complete! Your GHOST is running.                   │
│                                                           │
│  → Open Discord — your GHOST just sent you a message      │
│                                                           │
│  Manage services:   read the "services" skill             │
│  Add observability: services skill → observability extra  │
│  Add Tailscale:     services skill → tailscale extra      │
│  Reconfigure:       ghost init                            │
╰───────────────────────────────────────────────────────────╯
```

**Health probes** per service:

| Service      | Probe                                    | Timeout |
| ------------ | ---------------------------------------- | ------- |
| LLM provider | Already validated in Phase 1             | —       |
| llama-server | GET `http://127.0.0.1:11434/health`      | 5s      |
| SearXNG      | GET `http://127.0.0.1:8080`              | 5s      |
| Chrome       | GET `http://127.0.0.1:9222/json/version` | 5s      |
| Crawl4AI     | GET `http://127.0.0.1:11235/health`      | 5s      |
| Docling      | GET `http://127.0.0.1:5001/health`       | 5s      |

Services that fail health check: show warning (yellow), don't block. The daemon will
retry connections on its own.

**First Discord message**: After starting the daemon, poll its health endpoint for up to
30 seconds (1s interval). Once healthy, trigger a real LLM chat turn with a user message
like "Hello! I just finished setting up." The GHOST responds naturally — reading SOUL.md
and OPERATOR.md — producing its first real message in the Discord channel. If the daemon
doesn't become healthy within 30s or the chat turn fails, show a yellow warning ("Daemon
started but first message failed — check Discord manually") and continue. Don't fail
init.

## Onboarding Agent

An on-demand AI assistant available during the wizard by pressing `[h]` at any prompt.
Only available after Phase 1 (provider must be validated first).

### Behavior

- Opens a mini chat session in the terminal below the current prompt
- The user types questions, gets answers, presses `[q]` to return to the wizard
- Purely advisory — does not modify config, run commands, or skip steps
- Single or short multi-turn conversation per invocation
- Not available in non-interactive (fully scripted) mode

### System prompt

Bundled at `assets/onboarding-agent-prompt.md`. Contains:

- Role: "You are the GHOST onboarding assistant. Help the user understand the setup
  process and make decisions about their configuration."
- Current onboarding state: which phase, what's been configured, what's remaining
- Detection results: what's installed, what ports are active, platform info
- Service descriptions: what each service does, why it's needed, resource requirements
- Discord setup guide: the same step-by-step guide shown in Phase 2
- Config format reference: what goes in config.toml vs .env

The prompt is kept focused (~2000 tokens) — enough to be helpful, not so large it wastes
context on a small utility interaction.

### Dynamic context injection

Each time `[h]` is pressed, the agent receives the current wizard state:

```
## Current State
- Phase: 3 (Services)
- Current prompt: SearXNG setup
- Configured so far: provider=openrouter, model=claude-sonnet-4, discord=configured
- Remaining: SearXNG, Crawl4AI+Chrome, Docling
- Detected: podman available, no existing services running
```

This is injected as a system message alongside the static prompt.

## Services Skill

Bundled at `assets/skills/services/`.

### `skill.md` — Main skill

```yaml
---
name: services
description:
  Manage GHOST's infrastructure services. Use when you need to start, stop, restart, or
  troubleshoot any service (containers or native), check service health, or help the
  OPERATOR modify their service setup.
---
```

Content covers:

- **Architecture overview**: Two types of services:
  - Native (nix profile + systemd/launchd): ghost-daemon, llama-server, docling-serve
  - Containers (podman/docker compose): SearXNG, Crawl4AI, headless Chrome
- **File layout**:
  - `<workspace>/services/docker-compose.yml` — container stack
  - `<workspace>/services/searxng-settings.yml` — SearXNG config
  - `~/.config/systemd/user/*.service` — systemd units (Linux)
  - `~/Library/LaunchAgents/com.ghost.*.plist` — launchd plists (macOS)
- **Common operations** with exact commands:
  - Start/stop/restart containers:
    `podman compose -f <workspace>/services/docker-compose.yml up -d`
  - View container logs: `podman compose ... logs -f <service>`
  - Restart native services: `systemctl --user restart llama-server`
  - Check status: `systemctl --user status ghost-daemon llama-server docling-serve`
- **Health checking**: How to probe each service endpoint, expected responses
- **Adding a service**: Edit the compose file, run `podman compose up -d`
- **Removing a service**: Remove from compose file, run
  `podman compose up -d --remove-orphans`
- **Reconfiguring**: Run `ghost init` to re-run the wizard
- **Nix garbage collection**: Nix store grows over time as packages are updated. Include
  instructions for setting up automatic GC (`nix-collect-garbage -d` or
  `nix.settings.auto-optimise-store` via nix.conf)

### `observability.md` — Extra

Content covers:

- **What SigNoz provides**: Distributed tracing, metrics, and log aggregation for your
  GHOST via OpenTelemetry
- **Reference compose file**: A complete `docker-compose.signoz.yml` for running the
  SigNoz stack (ClickHouse + SigNoz OTel collector + SigNoz frontend), designed to be
  placed alongside the main compose file in `<workspace>/services/`
- **Configuration**: Set `OTEL_EXPORTER_OTLP_ENDPOINT=http://127.0.0.1:4317` in
  `~/.config/ghost/.env` to start sending traces
- **Managing the stack**: Start/stop commands, accessing the SigNoz UI (default :3301)
- **What to look for**: Key spans (chat turns, tool calls, agent runs, web fetches),
  example queries

### `tailscale.md` — Extra

Content covers:

- **What Tailscale provides**: Secure mesh VPN for remote access to your GHOST machine
  without opening ports or configuring firewalls
- **Installation**: Link to https://tailscale.com/download, one-line installer
- **Setup**: `tailscale up`, authenticate, verify connectivity
- **Exposing GHOST services**: Using `tailscale serve` to expose specific ports over the
  tailnet (not the public internet)
- **ACL considerations**: Restrict which devices can reach the GHOST machine

## Output Summary

A successful `ghost init` run produces:

```
~/.config/ghost/
├── config.toml              # All structured configuration
└── .env                     # Secrets (API keys, tokens)

~/GHOST/                     # Workspace (default path)
├── services/
│   ├── docker-compose.yml   # Container stack
│   └── searxng-settings.yml # SearXNG config (if selected)
├── skills/                  # Bundled skills (including services/)
├── agents/                  # Bundled agents
├── notes/                   # User notes
├── references/              # Reference topics
├── diary/                   # Daily logs
└── ...                      # Other workspace dirs

~/.config/systemd/user/      # Linux service units
├── ghost-daemon.service
├── llama-server.service     # If nix-installed
└── docling-serve.service    # If nix-installed

# Or macOS:
~/Library/LaunchAgents/
├── com.ghost.daemon.plist
├── com.ghost.llama-server.plist
└── com.ghost.docling-serve.plist
```
