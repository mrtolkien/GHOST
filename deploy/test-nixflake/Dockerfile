# Test: nix-native binary distribution via flake
#
# Strategy:
#   1. nixos/nix:latest base image
#   2. Copy pre-built ghost binary + flake into the image
#   3. `nix build` installs ghost (patched via autoPatchelfHook) + shell tools
#   4. Entrypoint rebuilds from flake if /nix is an empty volume
#
# NOTE: The locally-built binary may have a newer glibc than nix provides.
# Production builds would use a Debian-built binary (glibc 2.36) or we'd
# build inside nix. For this test we use a two-stage approach: build in
# Debian, then patch in nix.

# --- Stage 1: Build ghost in Debian (matching production glibc) ---
FROM lukemathwalker/cargo-chef:latest-rust-1 AS chef
WORKDIR /build

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
RUN apt-get update && \
    apt-get install -y --no-install-recommends pkg-config cmake && \
    rm -rf /var/lib/apt/lists/*
COPY --from=planner /build/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN cargo build --release && strip target/release/ghost

# --- Stage 2: Nix flake environment ---
FROM nixos/nix:latest

# Enable flakes
RUN echo "experimental-features = nix-command flakes" >> /etc/nix/nix.conf

# Copy flake + entrypoint into the image
COPY deploy/test-nixflake/flake.nix /workspace/shell/flake.nix
COPY deploy/test-nixflake/entrypoint.sh /opt/ghost/entrypoint.sh
RUN chmod +x /opt/ghost/entrypoint.sh

# Copy the Debian-built ghost binary where the flake expects it
RUN mkdir -p /workspace/shell/ghost-bin
COPY --from=builder /build/target/release/ghost /workspace/shell/ghost-bin/ghost

# Generate flake.lock and build everything (ghost + shell tools).
# This validates the flake works and populates the nix store.
WORKDIR /workspace/shell
RUN nix build . --no-link

# Set up PATH to point at the built environment.
# Create a symlink for ghost and store the env path for the entrypoint.
RUN STORE_PATH=$(nix build . --no-link --print-out-paths) && \
    mkdir -p /usr/local/bin && \
    ln -sf "${STORE_PATH}/bin/ghost" /usr/local/bin/ghost && \
    echo "${STORE_PATH}" > /opt/ghost/store-path

# Verify at build time
RUN /usr/local/bin/ghost --version

ENV GHOST_CONFIG_DIR=/config
ENV GHOST_WORKSPACE=/workspace

ENTRYPOINT ["/opt/ghost/entrypoint.sh"]
