#!/usr/bin/env sh
set -eu

FLAKE_DIR="/workspace/shell"
STORE_PATH_FILE="/opt/ghost/store-path"

# Fast path: check if the cached store path is still valid
if [ -f "$STORE_PATH_FILE" ]; then
    CACHED_PATH=$(cat "$STORE_PATH_FILE")
    if [ -x "${CACHED_PATH}/bin/ghost" ]; then
        export PATH="${CACHED_PATH}/bin:${PATH}"
        echo "[entrypoint] ghost ready (cached): $(ghost --version)"
        if [ $# -gt 0 ]; then exec "$@"; else exec ghost daemon; fi
    fi
fi

# Slow path: rebuild from flake
echo "[entrypoint] ghost not in store — rebuilding from flake..."
start=$(date +%s)
nix build "$FLAKE_DIR" --no-link
end=$(date +%s)
echo "[entrypoint] nix build completed in $((end - start))s"

# Resolve the store path and cache it
STORE_PATH=$(nix build "$FLAKE_DIR" --no-link --print-out-paths)
echo "${STORE_PATH}" > "$STORE_PATH_FILE" 2>/dev/null || true
export PATH="${STORE_PATH}/bin:${PATH}"

echo "[entrypoint] ghost version: $(ghost --version)"

if [ $# -gt 0 ]; then
    exec "$@"
else
    exec ghost daemon
fi
