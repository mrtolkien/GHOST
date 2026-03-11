#!/usr/bin/env sh
set -eu

FLAKE_DIR="/opt/ghost-flake"
STORE_CACHE="/opt/ghost/store-path"

# Fast path: cached store path has a working ghost binary
if [ -f "$STORE_CACHE" ]; then
    CACHED=$(cat "$STORE_CACHE")
    if [ -x "${CACHED}/bin/ghost" ]; then
        export PATH="${CACHED}/bin:${PATH}"
        echo "[ghost] ready (cached): $(ghost --version)"
        exec ghost daemon "$@"
    fi
fi

# Slow path: build from flake (first boot or after image update)
echo "[ghost] building from flake..."
STORE_PATH=$(nix build "$FLAKE_DIR" --no-link --print-out-paths)

mkdir -p "$(dirname "$STORE_CACHE")"
echo "$STORE_PATH" > "$STORE_CACHE"
export PATH="${STORE_PATH}/bin:${PATH}"
echo "[ghost] ready: $(ghost --version)"
exec ghost daemon "$@"
