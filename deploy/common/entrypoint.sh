#!/usr/bin/env sh
set -eu

ROOT_FLAKE="/opt/ghost-flake"
LIVE_FLAKE="${GHOST_WORKSPACE:-/workspace}/shell"

# Prefer live flake if it has been locked (includes ghost binary + shell tools)
if [ -f "${LIVE_FLAKE}/flake.nix" ] && [ -f "${LIVE_FLAKE}/flake.lock" ]; then
    FLAKE_DIR="$LIVE_FLAKE"
else
    FLAKE_DIR="$ROOT_FLAKE"
fi

echo "[ghost] building from flake (${FLAKE_DIR})..."
STORE_PATH=$(nix build "$FLAKE_DIR" --no-link --print-out-paths)
export PATH="${STORE_PATH}/bin:${PATH}"
echo "[ghost] ready: $(ghost --version)"
exec ghost daemon "$@"
