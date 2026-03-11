# Test: autoPatchelfHook approach for patching the ghost binary.
#
# Strategy:
#   1. Build ghost in Debian (matching production — glibc 2.36)
#   2. Use nixos/nix:latest to run autoPatchelfHook derivation
#   3. Copy patched binary + resolved libs to fixed paths OUTSIDE /nix
#   4. Verify the binary works even when /nix is overlaid by a volume
#
# NOTE: We use the full cargo-chef build here to test with a realistic binary.
# The locally-built binary (glibc 2.43) won't work because nix only has glibc 2.42.
# Production builds in Debian bookworm (glibc 2.36), which nix's 2.40+ covers.

# --- Stage 1: Build ghost (same as production) ---
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

# --- Stage 2: autoPatchelfHook in nix ---
FROM nixos/nix:latest

# Enable flakes
RUN echo "experimental-features = nix-command flakes" >> /etc/nix/nix.conf

# Copy the Debian-built ghost binary
COPY --from=builder /build/target/release/ghost /tmp/ghost-original

# Copy the nix derivation
COPY deploy/test-autopatchelf/ghost-bin.nix /tmp/ghost-bin.nix

# Pin nixpkgs channel
ARG NIXPKGS_URL="https://github.com/NixOS/nixpkgs/archive/nixos-24.11.tar.gz"
RUN nix-channel --add "${NIXPKGS_URL}" nixpkgs && nix-channel --update

# Build the patched binary using autoPatchelfHook
RUN nix-build /tmp/ghost-bin.nix \
      --arg ghostBinary /tmp/ghost-original \
      -o /tmp/ghost-patched

# Copy the patched binary to a fixed path outside /nix
RUN mkdir -p /usr/local/bin && \
    cp /tmp/ghost-patched/bin/ghost /usr/local/bin/ghost && \
    chmod +x /usr/local/bin/ghost

# Discover and copy all needed runtime libraries to /opt/ghost-libs/
# autoPatchelfHook already set the interpreter and rpath to nix store paths.
# We copy those libs to a fixed location and re-patch to use that path.
RUN mkdir -p /opt/ghost-libs && \
    # Get the interpreter path from the patched binary
    INTERP=$(nix-shell -p patchelf --run "patchelf --print-interpreter /usr/local/bin/ghost") && \
    echo "Interpreter: $INTERP" && \
    # Get the rpath
    RPATH=$(nix-shell -p patchelf --run "patchelf --print-rpath /usr/local/bin/ghost") && \
    echo "RPATH: $RPATH" && \
    # Copy the interpreter
    cp "$INTERP" /opt/ghost-libs/ld-linux-x86-64.so.2 && \
    # Copy all libs from rpath directories
    echo "$RPATH" | tr ':' '\n' | while read -r dir; do \
        if [ -d "$dir" ]; then \
            echo "Copying libs from $dir"; \
            cp -n "$dir"/*.so* /opt/ghost-libs/ 2>/dev/null || true; \
        fi; \
    done && \
    # Also need glibc libs (libc, libm, etc.) — interpreter's parent dir
    GLIBC_DIR=$(dirname "$INTERP") && \
    echo "Copying glibc libs from $GLIBC_DIR" && \
    cp -n "$GLIBC_DIR"/*.so* /opt/ghost-libs/ 2>/dev/null || true && \
    # Re-patch the binary to use our fixed paths
    nix-shell -p patchelf --run "\
        patchelf --set-interpreter /opt/ghost-libs/ld-linux-x86-64.so.2 \
                 --set-rpath /opt/ghost-libs \
                 /usr/local/bin/ghost" && \
    # List what we copied
    echo "=== Libraries in /opt/ghost-libs/ ===" && \
    ls -la /opt/ghost-libs/

# Verify the binary works right now
RUN echo "=== Test 1: Basic --version ===" && \
    /usr/local/bin/ghost --version

# Clean up build artifacts
RUN rm -f /tmp/ghost-original /tmp/ghost-bin.nix && \
    rm -f /tmp/ghost-patched

ENV PATH="/usr/local/bin:${PATH}"
ENV GHOST_CONFIG_DIR=/config
ENV GHOST_WORKSPACE=/workspace

CMD ["sh", "-c", "echo 'ghost binary at /usr/local/bin/ghost'; /usr/local/bin/ghost --version"]
