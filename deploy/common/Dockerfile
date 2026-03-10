# Stage 1: Generate dependency recipe
FROM lukemathwalker/cargo-chef:latest-rust-1 AS chef
WORKDIR /build

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# Stage 2: Build dependencies (cached) then build Ghost
FROM chef AS builder

RUN apt-get update && \
    apt-get install -y --no-install-recommends pkg-config cmake && \
    rm -rf /var/lib/apt/lists/*

# Cook dependencies — this layer is cached until Cargo.toml/Cargo.lock change
COPY --from=planner /build/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

# Build the actual binary
COPY . .
RUN cargo build --release && strip target/release/ghost

# Stage 3: Runtime with Nix
FROM nixos/nix:latest

# Enable flakes
RUN echo "experimental-features = nix-command flakes" >> /etc/nix/nix.conf

# Copy the Ghost binary
COPY --from=builder /build/target/release/ghost /usr/local/bin/ghost

# Patch the binary to use Nix's glibc (Debian-built ELF expects
# /lib64/ld-linux-x86-64.so.2 which doesn't exist in NixOS)
# Use tarball URL to avoid GitHub API rate limits in CI (github: flake refs
# resolve branches via api.github.com which gets 403 without a token)
ARG NIXPKGS="https://github.com/NixOS/nixpkgs/archive/nixos-24.11.tar.gz"
RUN nix shell "${NIXPKGS}#patchelf" --command sh -c "\
    GLIBC=\$(nix eval --raw \"${NIXPKGS}#glibc.out\") && \
    patchelf --set-interpreter \"\$GLIBC/lib/ld-linux-x86-64.so.2\" \
             --set-rpath \"\$GLIBC/lib\" \
             /usr/local/bin/ghost"

# Copy entrypoint
COPY deploy/common/entrypoint.sh /opt/ghost/entrypoint.sh

ENV PATH="/usr/local/bin:${PATH}"
ENV GHOST_CONFIG_DIR=/config
ENV GHOST_WORKSPACE=/workspace

ENTRYPOINT ["/opt/ghost/entrypoint.sh"]
