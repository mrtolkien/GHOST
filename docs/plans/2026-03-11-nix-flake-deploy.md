# Nix Flake Deployment Plan

> **For agentic workers:** REQUIRED: Use superpowers:executing-plans to implement
> this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the cargo-chef + patchelf Docker pipeline with a nix flake that
fetches the Ghost binary from GitHub Releases. No volume shadowing, no patchelf.

**Architecture:** Two flakes: root `flake.nix` (ghost binary only, baked into
Docker image) and `assets/shell/flake.nix` (runtime tools, Ghost-managed,
unchanged). CI builds native binaries, uploads to GitHub Releases, builds a minimal
`nixos/nix` Docker image. Entrypoint runs `nix build` on first boot to populate
the `/nix` volume.

---

## Dependency Tree

```
1. Bootstrap release (cargo build + gh release create)
2. Root flake (depends on 1 — URL must resolve to lock)
3. Dockerfile + entrypoint (depends on 2 — needs flake.lock)
4. CI workflow + workflow_dispatch test (depends on 3 — all files on branch)
5. Tagged release test (depends on 4 — full CI flow)
6. Cleanup (depends on 5 — flow proven)
```

---

## Task 1: Bootstrap release

Build locally, create a GitHub release so `/releases/latest/download/` resolves.

- [ ] **Step 1: Create branch**

```bash
git checkout -b feat/nix-flake-deploy
```

- [ ] **Step 2: Build the binary**

```bash
cargo build --release
strip target/release/ghost
```

- [ ] **Step 3: Create tarball**

```bash
mkdir -p /tmp/ghost-bin/x86_64
cp target/release/ghost /tmp/ghost-bin/x86_64/ghost
chmod +x /tmp/ghost-bin/x86_64/ghost
tar czf /tmp/ghost-bin.tar.gz -C /tmp/ghost-bin .
```

- [ ] **Step 4: Create GitHub release**

```bash
gh release create v0.1.0 /tmp/ghost-bin.tar.gz \
  --title "v0.1.0" \
  --notes "Bootstrap release for nix flake deployment."
```

- [ ] **Step 5: Verify the `/latest/` redirect resolves**

```bash
curl -sIL \
  "https://github.com/mrtolkien/GHOST/releases/latest/download/ghost-bin.tar.gz" \
  | grep -E "^HTTP/"
```

Expected: `HTTP/2 302` then `HTTP/2 200`.

---

## Task 2: Root flake

Write the flake and lock it against the bootstrap release.

- [ ] **Step 1: Write `flake.nix`**

```nix
{
  description = "Ghost binary package";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    ghost-bin = {
      url = "https://github.com/mrtolkien/GHOST/releases/latest/download/ghost-bin.tar.gz";
      flake = false;
    };
  };

  outputs = { self, nixpkgs, ghost-bin }:
    let
      supportedSystems = [ "x86_64-linux" "aarch64-linux" ];
      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
    in
    {
      packages = forAllSystems (system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          arch = if system == "x86_64-linux" then "x86_64" else "aarch64";
        in
        {
          default = pkgs.stdenv.mkDerivation {
            pname = "ghost";
            version = "bin";
            dontUnpack = true;

            nativeBuildInputs = [ pkgs.autoPatchelfHook ];
            buildInputs = [
              pkgs.glibc
              pkgs.gcc-unwrapped.lib
            ];

            installPhase = ''
              mkdir -p $out/bin
              cp ${ghost-bin}/${arch}/ghost $out/bin/ghost
              chmod +x $out/bin/ghost
            '';
          };
        }
      );
    };
}
```

- [ ] **Step 2: Lock the flake**

```bash
nix flake lock
```

Expected: creates `flake.lock` with hashes for `nixpkgs` and `ghost-bin`.

- [ ] **Step 3: Verify ghost binary works through nix**

```bash
STORE_PATH=$(nix build --no-link --print-out-paths)
$STORE_PATH/bin/ghost --version
```

Expected: prints ghost version.

- [ ] **Step 4: Commit**

```bash
git add flake.nix flake.lock
git commit -m "feat: root nix flake for ghost binary package"
```

---

## Task 3: Dockerfile + entrypoint

Rewrite both. The image is just `nixos/nix` + flake files.

- [ ] **Step 1: Rewrite `deploy/common/Dockerfile`**

```dockerfile
FROM nixos/nix:latest

RUN echo "experimental-features = nix-command flakes" >> /etc/nix/nix.conf

COPY flake.nix flake.lock /opt/ghost-flake/

COPY deploy/common/entrypoint.sh /opt/ghost/entrypoint.sh
RUN chmod +x /opt/ghost/entrypoint.sh

ENV GHOST_CONFIG_DIR=/config
ENV GHOST_WORKSPACE=/workspace

ENTRYPOINT ["/opt/ghost/entrypoint.sh"]
```

- [ ] **Step 2: Rewrite `deploy/common/entrypoint.sh`**

```sh
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
```

- [ ] **Step 3: Test Docker build**

```bash
docker build -f deploy/common/Dockerfile -t ghost:nix-test .
```

Expected: builds in seconds (just COPY + one RUN).

- [ ] **Step 4: Test Docker run**

```bash
docker run --rm -v nix-test-store:/nix ghost:nix-test ghost --version
```

Expected: nix build runs (~1-2 min first time), prints ghost version.

- [ ] **Step 5: Clean up test volume**

```bash
docker volume rm nix-test-store
```

- [ ] **Step 6: Commit**

```bash
git add deploy/common/Dockerfile deploy/common/entrypoint.sh
git commit -m "feat: minimal nix runtime Dockerfile + smart entrypoint"
```

---

## Task 4: CI workflow + workflow_dispatch test

Rewrite CI, push branch, verify docker job passes.

- [ ] **Step 1: Rewrite `.github/workflows/docker.yml`**

```yaml
name: Build & Release

on:
  push:
    branches: [main]
    paths:
      - 'flake.nix'
      - 'flake.lock'
      - 'deploy/common/**'
    tags: ['v*']
  workflow_dispatch:

jobs:
  build-binary:
    if: startsWith(github.ref, 'refs/tags/v')
    strategy:
      matrix:
        include:
          - runner: ubuntu-latest
            arch: x86_64
          - runner: ubuntu-24.04-arm
            arch: aarch64
    runs-on: ${{ matrix.runner }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: sudo apt-get update && sudo apt-get install -y pkg-config cmake
      - run: cargo build --release && strip target/release/ghost
      - uses: actions/upload-artifact@v4
        with:
          name: ghost-${{ matrix.arch }}
          path: target/release/ghost

  release:
    needs: build-binary
    runs-on: ubuntu-latest
    permissions:
      contents: write
    steps:
      - uses: actions/download-artifact@v4
      - run: |
          mkdir -p ghost-bin/x86_64 ghost-bin/aarch64
          cp ghost-x86_64/ghost ghost-bin/x86_64/ghost
          cp ghost-aarch64/ghost ghost-bin/aarch64/ghost
          chmod +x ghost-bin/*/ghost
          tar czf ghost-bin.tar.gz -C ghost-bin .
      - uses: softprops/action-gh-release@v2
        with:
          files: ghost-bin.tar.gz

  docker:
    needs: [release]
    if: always() && !cancelled() && needs.release.result != 'failure'
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: DeterminateSystems/nix-installer-action@main
      - name: Update flake.lock
        run: |
          if [[ "$GITHUB_REF" == refs/tags/v* ]]; then
            nix flake update
          fi
      - name: Verify flake.lock
        run: test -f flake.lock || { echo "::error::No flake.lock"; exit 1; }
      - uses: docker/setup-qemu-action@v3
      - uses: docker/setup-buildx-action@v3
      - uses: docker/login-action@v3
        with:
          username: ${{ secrets.DOCKERHUB_USERNAME }}
          password: ${{ secrets.DOCKERHUB_TOKEN }}
      - name: Tags
        id: tags
        run: |
          if [[ "$GITHUB_REF" == refs/tags/v* ]]; then
            VERSION="${GITHUB_REF#refs/tags/v}"
            echo "tags=mrtolkien/ghost:latest,mrtolkien/ghost:$VERSION" >> "$GITHUB_OUTPUT"
          else
            echo "tags=mrtolkien/ghost:nightly" >> "$GITHUB_OUTPUT"
          fi
      - uses: docker/build-push-action@v6
        with:
          context: .
          file: deploy/common/Dockerfile
          platforms: linux/amd64,linux/arm64
          push: true
          tags: ${{ steps.tags.outputs.tags }}
```

- [ ] **Step 2: Commit and push**

```bash
git add .github/workflows/docker.yml
git commit -m "feat: CI with native binary build + nix Docker image"
git push -u origin feat/nix-flake-deploy
```

- [ ] **Step 3: Trigger workflow_dispatch**

```bash
gh workflow run "Build & Release" --ref feat/nix-flake-deploy
```

- [ ] **Step 4: Monitor and verify**

```bash
gh run list --workflow docker.yml --limit 3
gh run watch <run-id>
```

Expected: `build-binary` and `release` skipped (not a tag). `docker` passes.

If it fails: `gh run view <run-id> --log-failed`, fix, push, re-trigger.

---

## Task 5: Tagged release test

Delete bootstrap release, tag the branch, verify full CI flow.

- [ ] **Step 1: Delete bootstrap release**

```bash
gh release delete v0.1.0 --yes
git push origin :refs/tags/v0.1.0
```

- [ ] **Step 2: Tag the branch**

```bash
git tag v0.1.0-rc1
git push origin v0.1.0-rc1
```

- [ ] **Step 3: Monitor full flow**

```bash
gh run list --workflow docker.yml --limit 3
gh run watch <run-id>
```

Expected: all 3 jobs pass (build-binary → release → docker).

- [ ] **Step 4: Verify release**

```bash
gh release view v0.1.0-rc1
```

Expected: `ghost-bin.tar.gz` listed as asset.

- [ ] **Step 5: Verify Docker image**

```bash
docker pull mrtolkien/ghost:0.1.0-rc1
docker run --rm -v nix-test-store:/nix mrtolkien/ghost:0.1.0-rc1 ghost --version
docker volume rm nix-test-store
```

Expected: ghost starts and prints version.

---

## Task 6: Cleanup

- [ ] **Step 1: Delete old test directories**

```bash
git rm -rf deploy/test-autopatchelf deploy/test-debian deploy/test-nixbuild deploy/test-nixflake
```

- [ ] **Step 2: Update flake.lock to CI release**

```bash
nix flake update
nix build --no-link --print-out-paths
```

Verify ghost binary from the CI-built release works.

- [ ] **Step 3: Commit**

```bash
git add flake.lock
git commit -m "chore: cleanup old deploy experiments, update flake.lock"
```

- [ ] **Step 4: Push**

```bash
git push
```
