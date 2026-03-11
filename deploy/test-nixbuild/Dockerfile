# Test: Build ghost entirely inside nixos/nix
# Result: Binary still needs patchelf because nix store hashes differ between
# builder and runtime stages, and /nix volume mounts break store paths anyway.
# Advantage over Debian build: simpler patchelf (nix glibc -> /opt/lib, no
# cross-distro mismatch).

ARG NIXPKGS="https://github.com/NixOS/nixpkgs/archive/nixos-unstable.tar.gz"

# ── Stage 1: Build ──────────────────────────────────────────────────────────
FROM nixos/nix:latest AS builder

ARG NIXPKGS

RUN echo "experimental-features = nix-command flakes" >> /etc/nix/nix.conf

# Install build toolchain via nix profile.
# Needs: rustc, cargo, gcc (mlua vendored C), pkg-config, cmake, gnumake, patchelf
RUN nix profile install \
      "${NIXPKGS}#rustc" \
      "${NIXPKGS}#cargo" \
      "${NIXPKGS}#gcc" \
      "${NIXPKGS}#pkg-config" \
      "${NIXPKGS}#cmake" \
      "${NIXPKGS}#gnumake" \
      "${NIXPKGS}#patchelf"

WORKDIR /build

# Copy manifests first for better layer caching
COPY Cargo.toml Cargo.lock ./

# Dummy src so cargo can resolve and build deps
RUN mkdir src && echo 'fn main() {}' > src/main.rs

# Pre-build dependencies (cached until Cargo.toml/Cargo.lock change)
RUN cargo build --release 2>&1 || true

# Copy real source and rebuild
COPY . .
RUN touch src/main.rs && cargo build --release && strip target/release/ghost

# Collect the runtime libs the binary needs into /opt/lib (outside /nix).
# This makes the binary survive /nix volume mounts.
# Use patchelf --print-* (no need for readelf/sed/awk which aren't in nix minimal).
RUN mkdir -p /opt/lib && \
    INTERP=$(patchelf --print-interpreter target/release/ghost) && \
    RPATH=$(patchelf --print-rpath target/release/ghost) && \
    echo "INTERP: $INTERP" && \
    echo "RPATH: $RPATH" && \
    # Copy the dynamic linker
    cp -aL "$INTERP" /opt/lib/ && \
    # Copy all libs from RPATH dirs
    for dir in $(echo "$RPATH" | tr ':' ' '); do \
      if [ -d "$dir" ]; then \
        echo "Copying libs from $dir" && \
        cp -aL "$dir"/*.so* /opt/lib/ 2>/dev/null || true; \
      fi; \
    done && \
    # Patch the binary to use /opt/lib
    patchelf --set-interpreter /opt/lib/ld-linux-x86-64.so.2 \
             --set-rpath /opt/lib \
             target/release/ghost && \
    # Verify
    echo "Patched:" && \
    patchelf --print-interpreter target/release/ghost && \
    patchelf --print-rpath target/release/ghost && \
    ls -la /opt/lib/

# ── Stage 2: Runtime ────────────────────────────────────────────────────────
FROM nixos/nix:latest

RUN echo "experimental-features = nix-command flakes" >> /etc/nix/nix.conf

# Copy the patched binary and its runtime libs
COPY --from=builder /opt/lib /opt/lib
COPY --from=builder /build/target/release/ghost /usr/local/bin/ghost

# Copy entrypoint
COPY deploy/common/entrypoint.sh /opt/ghost/entrypoint.sh

ENV PATH="/usr/local/bin:${PATH}"
ENV GHOST_CONFIG_DIR=/config
ENV GHOST_WORKSPACE=/workspace

ENTRYPOINT ["/opt/ghost/entrypoint.sh"]
