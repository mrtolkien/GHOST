#!/bin/sh
set -e

WORKSPACE="${GHOST_WORKSPACE:-/workspace}"

# Ensure workspace shell directory exists with default flake
mkdir -p "$WORKSPACE/shell"
if [ ! -f "$WORKSPACE/shell/flake.nix" ]; then
  cp /opt/ghost/default-flake.nix "$WORKSPACE/shell/flake.nix"
fi

# Bootstrap home-manager environment (installs all packages).
# Fast if already cached in /nix volume.
echo "Running home-manager switch..."
nix run home-manager -- switch --flake "$WORKSPACE/shell/#ghost"

# Source home-manager profile for PATH
export PATH="$HOME/.nix-profile/bin:$PATH"

exec /usr/local/bin/ghost daemon "$@"
