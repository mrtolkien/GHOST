#!/usr/bin/env bash
set -euo pipefail

GHOST_REPO="mrtolkien/ghost"
GHOST_BRANCH="main"
GHOST_RAW="https://raw.githubusercontent.com/${GHOST_REPO}/${GHOST_BRANCH}"
GHOST_CONFIG_DIR="${HOME}/.config/ghost"
GHOST_DATA_DIR="${HOME}/.local/share/ghost"
GHOST_LOG_DIR="${GHOST_DATA_DIR}/logs"
MODEL_DIR="${GHOST_DATA_DIR}/models"
LAUNCHD_DIR="${HOME}/Library/LaunchAgents"

# Embedding model — Qwen3 Embedding 8B Q8_0 GGUF
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
    if nix profile list | grep -q "${pkg}"; then
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
    if ! nix profile list | grep -q "colima"; then
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
