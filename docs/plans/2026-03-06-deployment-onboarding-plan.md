# Deployment & Onboarding Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this
> plan task-by-task.

**Goal:** Single `curl | sh` install for the full GHOST stack on macOS Apple Silicon.

**Architecture:** Nix installs all packages (llama-cpp, docling-serve, docker, colima).
launchd runs GPU services natively with Metal. Docker runs ghost + crawl4ai + searxng. A
Python onboarding wizard (`questionary`) handles interactive config.

**Tech Stack:** Shell (install script), Python + questionary (onboarding), launchd
(plists), Docker Compose, Nix.

**Design doc:** `docs/plans/2026-03-06-deployment-onboarding-design.md`

---

### Task 1: Create deploy/ directory structure and move Dockerfile

Move existing Docker files to `deploy/common/`, update all references.

**Files:**

- Move: `docker/Dockerfile` → `deploy/common/Dockerfile`
- Move: `docker/entrypoint.sh` → `deploy/common/entrypoint.sh`
- Move: `docker/default-flake.nix` → `deploy/common/default-flake.nix`
- Move: `docker/searxng-settings.yml` → `deploy/common/searxng-settings.yml`
- Modify: `.github/workflows/docker.yml:35` — update `file:` path
- Modify: `docker-compose.local.yml:5` — update `dockerfile:` path
- Modify: `docker-compose.yml:26` — update searxng volume mount path
- Modify: `Dockerfile` itself — update `COPY` paths for entrypoint/flake
- Create: `deploy/macos/` directory
- Create: `deploy/linux/` directory (empty, with `.gitkeep`)

**Step 1: Create deploy directories**

```bash
mkdir -p deploy/common deploy/macos deploy/linux
```

**Step 2: Move files**

```bash
git mv docker/Dockerfile deploy/common/Dockerfile
git mv docker/entrypoint.sh deploy/common/entrypoint.sh
git mv docker/default-flake.nix deploy/common/default-flake.nix
git mv docker/searxng-settings.yml deploy/common/searxng-settings.yml
rmdir docker
touch deploy/linux/.gitkeep
```

**Step 3: Update Dockerfile COPY paths**

In `deploy/common/Dockerfile`, the COPY lines reference `docker/` — update to
`deploy/common/`:

```dockerfile
COPY deploy/common/default-flake.nix /opt/ghost/default-flake.nix
COPY deploy/common/entrypoint.sh /opt/ghost/entrypoint.sh
```

**Step 4: Update docker-compose.yml**

In root `docker-compose.yml`, update searxng volume mount:

```yaml
- ./deploy/common/searxng-settings.yml:/etc/searxng/settings.yml:ro
```

**Step 5: Update docker-compose.local.yml**

```yaml
dockerfile: deploy/common/Dockerfile
```

**Step 6: Update GitHub Actions workflow**

In `.github/workflows/docker.yml:35`:

```yaml
file: deploy/common/Dockerfile
```

**Step 7: Verify Docker build still works**

```bash
docker compose -f docker-compose.yml -f docker-compose.local.yml build
```

**Step 8: Commit**

```bash
git add -A
git commit -m "refactor: move docker files to deploy/common/"
```

---

### Task 2: Create macOS Docker Compose file

Self-contained compose file for the installed macOS setup. No docling (runs natively).
Ghost connects to host services via `host.docker.internal`.

**Files:**

- Create: `deploy/macos/docker-compose.yml`

**Step 1: Write the compose file**

```yaml
services:
  ghost:
    image: mrtolkien/ghost:latest
    volumes:
      - ${GHOST_WORKSPACE:-~/GHOST}:/workspace
      - ${GHOST_CONFIG:-~/.config/ghost}:/config
      - nix-store:/nix
    env_file: ${GHOST_CONFIG:-~/.config/ghost}/.env
    environment:
      - GHOST_CONFIG_DIR=/config
      - GHOST_WORKSPACE=/workspace
      - CRAWL4AI_URL=http://crawl4ai:11235
      - SEARXNG_URL=http://searxng:8080
      - DOCLING_URL=http://host.docker.internal:5001
    extra_hosts:
      - "host.docker.internal:host-gateway"
    networks:
      - ghost-net
    restart: unless-stopped

  crawl4ai:
    image: unclecode/crawl4ai:latest
    networks:
      - ghost-net
    restart: unless-stopped

  searxng:
    image: searxng/searxng:latest
    volumes:
      - ${GHOST_CONFIG:-~/.config/ghost}/searxng-settings.yml:/etc/searxng/settings.yml:ro
    networks:
      - ghost-net
    restart: unless-stopped

volumes:
  nix-store:

networks:
  ghost-net:
```

Note: `env_file` points to the config directory (not CWD). `extra_hosts` ensures
`host.docker.internal` resolves on both Docker Desktop and Colima. The searxng settings
file is loaded from the ghost config dir (curled there during install).

**Step 2: Commit**

```bash
git add deploy/macos/docker-compose.yml
git commit -m "feat: add macOS-specific docker-compose for installed setup"
```

---

### Task 3: Create launchd plist templates

Plist files for llama-server and docling-serve. These are templates — the install script
substitutes `__MODEL_PATH__` and `__LLAMA_SERVER_BIN__` etc. at install time.

**Files:**

- Create: `deploy/macos/com.ghost.llama-server.plist`
- Create: `deploy/macos/com.ghost.docling-serve.plist`

**Step 1: Write llama-server plist**

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>com.ghost.llama-server</string>
  <key>ProgramArguments</key>
  <array>
    <string>__LLAMA_SERVER_BIN__</string>
    <string>--model</string>
    <string>__MODEL_PATH__</string>
    <string>--embedding</string>
    <string>--port</string>
    <string>11434</string>
    <string>--host</string>
    <string>0.0.0.0</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>StandardOutPath</key>
  <string>__LOG_DIR__/llama-server.log</string>
  <key>StandardErrorPath</key>
  <string>__LOG_DIR__/llama-server.err</string>
</dict>
</plist>
```

**Step 2: Write docling-serve plist**

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>com.ghost.docling-serve</string>
  <key>ProgramArguments</key>
  <array>
    <string>__DOCLING_SERVE_BIN__</string>
    <string>--host</string>
    <string>0.0.0.0</string>
    <string>--port</string>
    <string>5001</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>StandardOutPath</key>
  <string>__LOG_DIR__/docling-serve.log</string>
  <key>StandardErrorPath</key>
  <string>__LOG_DIR__/docling-serve.err</string>
</dict>
</plist>
```

**Step 3: Commit**

```bash
git add deploy/macos/com.ghost.llama-server.plist deploy/macos/com.ghost.docling-serve.plist
git commit -m "feat: add launchd plist templates for llama-server and docling-serve"
```

---

### Task 4: Create the onboarding Python script

Interactive config wizard. PEP 723 inline deps. Writes `.env` and `config.toml`.

**Files:**

- Create: `deploy/common/onboard.py`

**Step 1: Write the onboarding script**

```python
# /// script
# requires-python = ">=3.11"
# dependencies = [
#     "questionary>=2.1.0",
# ]
# ///
"""GHOST interactive onboarding wizard."""

from pathlib import Path
import sys

import questionary

GHOST_CONFIG_DIR = Path.home() / ".config" / "ghost"

PROVIDERS = {
    "OpenRouter": {
        "env_key": "OPENROUTER_API_KEY",
        "config_name": "openrouter",
        "models": [
            ("anthropic/claude-sonnet-4", 200_000),
            ("anthropic/claude-haiku-4", 200_000),
            ("google/gemini-2.5-pro-preview", 1_000_000),
            ("google/gemini-2.5-flash-preview", 1_000_000),
            ("deepseek/deepseek-r1", 64_000),
        ],
    },
    "Kimi": {
        "env_key": "KIMI_API_KEY",
        "config_name": "kimi",
        "models": [
            ("kimi-k2", 128_000),
        ],
    },
    "OpenAI (OAuth — free, uses ChatGPT account)": {
        "env_key": None,  # OAuth flow, no API key
        "config_name": "openai_oauth",
        "models": [
            ("o4-mini", 200_000),
            ("gpt-4.1", 1_000_000),
        ],
    },
}


def select_provider() -> dict:
    provider_name = questionary.select(
        "Select your LLM provider:",
        choices=list(PROVIDERS.keys()),
    ).ask()
    if provider_name is None:
        sys.exit(1)
    return {**PROVIDERS[provider_name], "display_name": provider_name}


def get_api_key(provider: dict) -> str | None:
    if provider["env_key"] is None:
        questionary.print(
            "  OpenAI OAuth uses your ChatGPT account — no API key needed.",
            style="italic",
        )
        questionary.print(
            "  Run `ghost auth openai` after install to complete OAuth login.",
            style="italic",
        )
        return None

    api_key = questionary.text(
        f"Enter your {provider['display_name']} API key:",
        validate=lambda v: len(v.strip()) > 0 or "API key cannot be empty",
    ).ask()
    if api_key is None:
        sys.exit(1)
    return api_key.strip()


def select_model(provider: dict) -> tuple[str, int]:
    choices = [f"{name} ({ctx // 1000}k ctx)" for name, ctx in provider["models"]]
    selection = questionary.select(
        "Select your default model:", choices=choices
    ).ask()
    if selection is None:
        sys.exit(1)
    idx = choices.index(selection)
    return provider["models"][idx]


def get_discord_config() -> tuple[str, str]:
    token = questionary.text(
        "Enter your Discord bot token:",
        validate=lambda v: len(v.strip()) > 0 or "Token cannot be empty",
    ).ask()
    if token is None:
        sys.exit(1)

    user_id = questionary.text(
        "Enter your Discord user ID:",
        validate=lambda v: v.strip().isdigit() or "User ID must be numeric",
    ).ask()
    if user_id is None:
        sys.exit(1)

    return token.strip(), user_id.strip()


def write_env(api_key: str | None, env_key: str | None, discord_token: str) -> None:
    GHOST_CONFIG_DIR.mkdir(parents=True, exist_ok=True)
    env_path = GHOST_CONFIG_DIR / ".env"

    lines = []
    if api_key and env_key:
        lines.append(f"{env_key}={api_key}")
    lines.append(f"DISCORD_TOKEN={discord_token}")

    env_path.write_text("\n".join(lines) + "\n")
    questionary.print(f"  Wrote {env_path}", style="bold")


def write_config(
    provider_name: str,
    model: str,
    context_window: int,
    discord_user_id: str,
    embeddings_url: str,
) -> None:
    GHOST_CONFIG_DIR.mkdir(parents=True, exist_ok=True)
    config_path = GHOST_CONFIG_DIR / "config.toml"

    config = f"""\
[discord]
allowed_user_id = "{discord_user_id}"

[models.primary]
provider = "{provider_name}"
model = "{model}"
context_window = {context_window}

[embeddings]
url = "{embeddings_url}"
model = "qwen3-embedding:8b"

[web]
crawl4ai_url = "http://crawl4ai:11235"
docling_url = "http://host.docker.internal:5001"

[web.search]
provider = "searxng"
url = "http://searxng:8080"
"""
    config_path.write_text(config)
    questionary.print(f"  Wrote {config_path}", style="bold")


def main() -> None:
    questionary.print("\n  Welcome to GHOST setup!\n", style="bold")

    provider = select_provider()
    api_key = get_api_key(provider)
    model, context_window = select_model(provider)
    discord_token, discord_user_id = get_discord_config()

    questionary.print("\n  Writing configuration...\n", style="bold")

    write_env(api_key, provider["env_key"], discord_token)

    # Detect if running inside macOS install (embeddings on host) vs container
    embeddings_url = "http://host.docker.internal:11434"

    write_config(
        provider["config_name"],
        model,
        context_window,
        discord_user_id,
        embeddings_url,
    )

    questionary.print("\n  GHOST configuration complete!\n", style="bold fg:green")


if __name__ == "__main__":
    main()
```

**Step 2: Test the script runs**

```bash
uv run deploy/common/onboard.py
```

Verify: interactive prompts appear, selecting options works, files are written to
`~/.config/ghost/`. Clean up test files after.

**Step 3: Commit**

```bash
git add deploy/common/onboard.py
git commit -m "feat: add interactive onboarding wizard"
```

---

### Task 5: Create the macOS install script

Shell script that orchestrates the full install. Idempotent — safe to re-run.

**Files:**

- Create: `deploy/macos/install.sh`

**Step 1: Write the install script**

```bash
#!/usr/bin/env bash
set -euo pipefail

GHOST_REPO="mrtolkien/ghost"
GHOST_BRANCH="main"
GHOST_RAW="https://raw.githubusercontent.com/${GHOST_REPO}/${GHOST_BRANCH}"
GHOST_CONFIG_DIR="${HOME}/.config/ghost"
GHOST_DATA_DIR="${HOME}/.local/share/ghost"
GHOST_LOG_DIR="${HOME}/.local/share/ghost/logs"
MODEL_DIR="${GHOST_DATA_DIR}/models"
LAUNCHD_DIR="${HOME}/Library/LaunchAgents"

# Embedding model — qwen3-embedding 8B Q8_0 GGUF
MODEL_FILENAME="Qwen3-Embedding-8B-Q8_0.gguf"
MODEL_URL="https://huggingface.co/Qwen/Qwen3-Embedding-8B-GGUF/resolve/main/${MODEL_FILENAME}"

info()  { printf '\033[1;34m==>\033[0m \033[1m%s\033[0m\n' "$*"; }
warn()  { printf '\033[1;33m==> WARNING:\033[0m %s\n' "$*"; }
error() { printf '\033[1;31m==> ERROR:\033[0m %s\n' "$*" >&2; exit 1; }

# ── 0. Platform check ────────────────────────────────────────────────────
[[ "$(uname)" == "Darwin" ]] || error "This script is for macOS only."

# ── 1. Install Nix ───────────────────────────────────────────────────────
if command -v nix &>/dev/null; then
    info "Nix already installed"
else
    info "Installing Nix (Determinate Systems installer)..."
    curl --proto '=https' --tlsv1.2 -sSf -L \
        https://install.determinate.systems/nix | sh -s -- install --no-confirm

    # Source nix profile so it's available in this script
    if [[ -f /nix/var/nix/profiles/default/etc/profile.d/nix-daemon.sh ]]; then
        . /nix/var/nix/profiles/default/etc/profile.d/nix-daemon.sh
    fi
fi

# ── 2. Install packages via nix ──────────────────────────────────────────
info "Installing packages via nix..."

NIX_PKGS=(llama-cpp docling-serve docker-client docker-compose uv)

for pkg in "${NIX_PKGS[@]}"; do
    if nix profile list | grep -q "nixpkgs#${pkg}"; then
        info "  ${pkg} already installed"
    else
        info "  Installing ${pkg}..."
        nix profile install "nixpkgs#${pkg}"
    fi
done

# ── 3. Docker runtime ───────────────────────────────────────────────────
if docker info &>/dev/null; then
    info "Docker runtime available (Docker Desktop or existing Colima)"
else
    info "No Docker runtime detected — installing Colima..."
    if ! nix profile list | grep -q "nixpkgs#colima"; then
        nix profile install nixpkgs#colima
    fi
    info "Starting Colima..."
    colima start
fi

# ── 4. Download embedding model ──────────────────────────────────────────
mkdir -p "${MODEL_DIR}"
MODEL_PATH="${MODEL_DIR}/${MODEL_FILENAME}"

if [[ -f "${MODEL_PATH}" ]]; then
    info "Embedding model already downloaded"
else
    info "Downloading embedding model (this may take a few minutes)..."
    curl -L --progress-bar -o "${MODEL_PATH}" "${MODEL_URL}"
fi

# ── 5. Register launchd services ─────────────────────────────────────────
mkdir -p "${LAUNCHD_DIR}" "${GHOST_LOG_DIR}"

install_launchd_service() {
    local label="$1" template_url="$2"
    local plist_path="${LAUNCHD_DIR}/${label}.plist"

    # Unload if already loaded
    launchctl bootout "gui/$(id -u)/${label}" 2>/dev/null || true

    info "Installing ${label}..."
    curl -sSf "${template_url}" -o "${plist_path}"

    # Substitute template variables
    local llama_bin docling_bin
    llama_bin="$(which llama-server)"
    docling_bin="$(which docling-serve)"

    sed -i '' "s|__LLAMA_SERVER_BIN__|${llama_bin}|g" "${plist_path}"
    sed -i '' "s|__DOCLING_SERVE_BIN__|${docling_bin}|g" "${plist_path}"
    sed -i '' "s|__MODEL_PATH__|${MODEL_PATH}|g" "${plist_path}"
    sed -i '' "s|__LOG_DIR__|${GHOST_LOG_DIR}|g" "${plist_path}"

    launchctl bootstrap "gui/$(id -u)" "${plist_path}"
}

install_launchd_service "com.ghost.llama-server" \
    "${GHOST_RAW}/deploy/macos/com.ghost.llama-server.plist"
install_launchd_service "com.ghost.docling-serve" \
    "${GHOST_RAW}/deploy/macos/com.ghost.docling-serve.plist"

# ── 6. Interactive onboarding ─────────────────────────────────────────────
info "Running onboarding wizard..."
ONBOARD_SCRIPT="/tmp/ghost-onboard.py"
curl -sSf "${GHOST_RAW}/deploy/common/onboard.py" -o "${ONBOARD_SCRIPT}"
uv run "${ONBOARD_SCRIPT}"
rm -f "${ONBOARD_SCRIPT}"

# ── 7. Docker compose up ─────────────────────────────────────────────────
info "Starting Docker services..."
mkdir -p "${GHOST_CONFIG_DIR}"

curl -sSf "${GHOST_RAW}/deploy/macos/docker-compose.yml" \
    -o "${GHOST_CONFIG_DIR}/docker-compose.yml"
curl -sSf "${GHOST_RAW}/deploy/common/searxng-settings.yml" \
    -o "${GHOST_CONFIG_DIR}/searxng-settings.yml"

docker compose -f "${GHOST_CONFIG_DIR}/docker-compose.yml" up -d

# ── Done ──────────────────────────────────────────────────────────────────
info "GHOST is running!"
info ""
info "  Config:  ${GHOST_CONFIG_DIR}/config.toml"
info "  Compose: ${GHOST_CONFIG_DIR}/docker-compose.yml"
info "  Logs:    ${GHOST_LOG_DIR}/"
info "  Model:   ${MODEL_PATH}"
info ""
info "  To stop:    docker compose -f ${GHOST_CONFIG_DIR}/docker-compose.yml down"
info "  To restart: docker compose -f ${GHOST_CONFIG_DIR}/docker-compose.yml up -d"
```

**Step 2: Make executable**

```bash
chmod +x deploy/macos/install.sh
```

**Step 3: Commit**

```bash
git add deploy/macos/install.sh
git commit -m "feat: add macOS install script"
```

---

### Task 6: Update root docker-compose.yml env_file path

The root dev compose currently uses `env_file: .env` (root of repo). The macOS compose
uses `${GHOST_CONFIG_DIR}/.env`. Make sure the root dev compose still works (it already
does — `.env` is relative to CWD which is the repo root).

No changes needed to root compose. This task is just verification.

**Step 1: Verify dev compose still works**

```bash
docker compose config
```

Expected: valid compose config printed, no errors.

**Step 2: Verify macOS compose renders correctly**

```bash
GHOST_CONFIG=$HOME/.config/ghost docker compose -f deploy/macos/docker-compose.yml config
```

Expected: valid compose config with `host.docker.internal` in env vars.

---

### Task 7: Update documentation

Update the installation docs page to reference the new install method.

**Files:**

- Modify: `docs/src/content/docs/getting-started/installation.mdx`

**Step 1: Read the `/docs` skill for formatting conventions**

Invoke the docs skill before editing.

**Step 2: Rewrite installation.mdx**

The page should lead with the single-command install, then explain what it does, then
cover manual/dev setup as an alternative. Keep existing "Building locally" section.

**Step 3: Commit**

```bash
git add docs/src/content/docs/getting-started/installation.mdx
git commit -m "docs: update installation guide for macOS one-line install"
```

---

### Task 8: Final verification and cleanup

**Step 1: Remove empty docker/ directory if it still exists**

```bash
[ -d docker ] && rmdir docker
```

**Step 2: Verify the full directory structure**

```bash
find deploy/ -type f | sort
```

Expected:

```
deploy/common/Dockerfile
deploy/common/default-flake.nix
deploy/common/entrypoint.sh
deploy/common/onboard.py
deploy/common/searxng-settings.yml
deploy/macos/com.ghost.docling-serve.plist
deploy/macos/com.ghost.llama-server.plist
deploy/macos/docker-compose.yml
deploy/macos/install.sh
deploy/linux/.gitkeep
```

**Step 3: Run CI**

```bash
just ci
```

Expected: all checks pass (this doesn't test the install script, but ensures nothing in
the Rust codebase broke from the file moves).

**Step 4: Commit any remaining changes**

```bash
git add -A
git commit -m "chore: cleanup after deploy restructure"
```
