# =============================================================================
# Stage 1: Build ghost on Debian so glibc matches the runtime
# =============================================================================
FROM rust:1-bookworm AS builder
WORKDIR /build

RUN apt-get update && \
    apt-get install -y --no-install-recommends pkg-config cmake && \
    rm -rf /var/lib/apt/lists/*

COPY . .
RUN cargo build --release && strip target/release/ghost

# =============================================================================
# Stage 2: Debian runtime with Nix
# =============================================================================
FROM debian:bookworm-slim

# Install deps needed for nix installer + runtime
RUN apt-get update && \
    apt-get install -y --no-install-recommends \
        curl xz-utils ca-certificates sudo git rsync \
    && rm -rf /var/lib/apt/lists/*

# Install nix via DeterminateSystems installer — designed for Docker / CI.
# --init none: no systemd/launchd init (Docker has no init system)
RUN curl --proto '=https' --tlsv1.2 -sSf -L \
        https://install.determinate.systems/nix | \
    sh -s -- install linux --no-confirm --init none

# Snapshot the nix installation so it can be restored when a /nix volume is mounted.
# The volume mount shadows everything installed by the installer, so we keep a backup
# at /opt/nix-snapshot that gets rsync'd into the volume on first boot.
RUN cp -a /nix /opt/nix-snapshot

# Put nix on PATH for all subsequent RUN and container commands
ENV PATH="/root/.nix-profile/bin:/nix/var/nix/profiles/default/bin:${PATH}"

# Flakes are enabled by default with DeterminateSystems installer, but be explicit
RUN mkdir -p /root/.config/nix && \
    echo "experimental-features = nix-command flakes" > /root/.config/nix/nix.conf

# Copy the Debian-built ghost binary — no patchelf needed
COPY --from=builder /build/target/release/ghost /usr/local/bin/ghost

# Copy the shell flake so we can test nix build
RUN mkdir -p /workspace/shell
COPY assets/shell/flake.nix /workspace/shell/flake.nix

# Startup script: restore nix into volume if it's empty, then exec the command
COPY deploy/test-debian/entrypoint.sh /opt/ghost/entrypoint.sh
RUN chmod +x /opt/ghost/entrypoint.sh

ENV GHOST_CONFIG_DIR=/config
ENV GHOST_WORKSPACE=/workspace

ENTRYPOINT ["/opt/ghost/entrypoint.sh"]
CMD ["sh"]
