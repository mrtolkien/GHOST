#!/bin/sh
set -e

# If /nix/store is empty (volume mount shadowed it), restore from snapshot.
# This handles the case where a fresh /nix volume is mounted.
if [ ! -d /nix/store ] || [ -z "$(ls -A /nix/store 2>/dev/null)" ]; then
    echo "[ghost] Restoring nix into volume..."
    rsync -a /opt/nix-snapshot/ /nix/
    echo "[ghost] Nix restored."
fi

exec "$@"
