# Nix Shell Improvements Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace per-command `nix develop` wrapping with home-manager as the system
shell manager, and make the ghost binary a Nix flake input for self-update without image
rebuilds.

**Architecture:** CI publishes release binaries to GitHub. A top-level `flake.nix`
defines a ghost package that fetches those binaries. The workspace `shell/flake.nix`
uses home-manager with ghost as an input. The daemon runs `home-manager switch` on boot
and the GHOST runs it via shell commands after editing the flake. Shell commands run
without `nix develop` wrapping — packages are in PATH via home-manager's profile.

**Tech Stack:** Nix flakes, home-manager (standalone), GitHub Actions, patchelf

**Design doc:** `docs/plans/2026-03-08-nix-shell-improvements-design.md`

---

### Task 1: CI — Extract binary build into standalone job

Split the binary build out of the Docker multi-stage build into its own CI job that
uploads the binary as a workflow artifact. The Docker job then pulls the artifact instead
of building from source.

**Files:**
- Modify: `.github/workflows/docker.yml`

**Step 1: Add `build-binary` job with matrix**

Add a new job before the existing `build` job. Uses the same runner matrix (x64 + arm
native builders). Installs Rust, builds the binary, uploads as artifact.

```yaml
  build-binary:
    strategy:
      matrix:
        include:
          - platform: linux/amd64
            runner: ubuntu-latest
            target: x86_64-unknown-linux-gnu
            artifact: ghost-x86_64-linux
          - platform: linux/arm64
            runner: ubuntu-24.04-arm
            target: aarch64-unknown-linux-gnu
            artifact: ghost-aarch64-linux
    runs-on: ${{ matrix.runner }}
    steps:
      - uses: actions/checkout@v4

      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.target }}

      - name: Install build deps
        run: sudo apt-get update && sudo apt-get install -y pkg-config cmake

      - name: Build release binary
        run: cargo build --release --target ${{ matrix.target }} && strip target/${{ matrix.target }}/release/ghost

      - uses: actions/upload-artifact@v4
        with:
          name: ${{ matrix.artifact }}
          path: target/${{ matrix.target }}/release/ghost
          retention-days: 7
```

**Step 2: Simplify Docker `build` job to use pre-built binary**

Update the existing `build` job to:
1. Depend on `build-binary`
2. Download the binary artifact
3. Pass it to the Docker build via `COPY` from a local path

Add `needs: build-binary` and download the artifact before Docker build:

```yaml
  build:
    needs: build-binary
    # ... existing matrix ...
    steps:
      - uses: actions/checkout@v4

      - name: Determine artifact name
        id: artifact
        run: |
          if [[ "${{ matrix.platform }}" == "linux/amd64" ]]; then
            echo "name=ghost-x86_64-linux" >> "$GITHUB_OUTPUT"
          else
            echo "name=ghost-aarch64-linux" >> "$GITHUB_OUTPUT"
          fi

      - uses: actions/download-artifact@v4
        with:
          name: ${{ steps.artifact.outputs.name }}
          path: deploy/common/ghost-bin/

      - name: Make binary executable
        run: chmod +x deploy/common/ghost-bin/ghost

      # ... existing Docker build steps ...
```

**Step 3: Verify workflow runs**

Push to a branch and check GitHub Actions.
Expected: binary builds on both architectures, Docker build uses the artifacts.

**Step 4: Commit**

```
feat: extract binary build into standalone CI job
```

---

### Task 2: CI — Add release upload job

On `v*` tags, attach the built binaries to the GitHub Release so the Nix flake can
fetch them by URL + hash.

**Files:**
- Modify: `.github/workflows/docker.yml`

**Step 1: Add `release` job**

```yaml
  release:
    if: startsWith(github.ref, 'refs/tags/v')
    needs: build-binary
    runs-on: ubuntu-latest
    permissions:
      contents: write
    steps:
      - uses: actions/download-artifact@v4
        with:
          pattern: ghost-*
          merge-multiple: false

      - name: Upload binaries to release
        env:
          GH_TOKEN: ${{ github.token }}
        run: |
          TAG="${GITHUB_REF#refs/tags/}"
          for dir in ghost-*/; do
            name=$(basename "$dir")
            cp "$dir/ghost" "$name"
            gh release upload "$TAG" "$name" --repo "${{ github.repository }}" --clobber
          done
```

**Step 2: Test with a draft release**

Create a test tag and verify binaries appear on the release page.

**Step 3: Commit**

```
feat: upload release binaries to GitHub Releases
```

---

### Task 3: Ghost Nix package flake

Create a top-level `flake.nix` that defines the ghost package. This is what the
workspace flake references as an input.

**Files:**
- Create: `flake.nix` (repo root)
- Create: `nix/package.nix` (package derivation)

**Step 1: Create `nix/package.nix`**

Nix function that builds the ghost package from a pre-built binary.
Uses `autoPatchelfHook` to fix the glibc dynamic linker (same logic currently in
the Dockerfile at `deploy/common/Dockerfile:35-39`).

```nix
# nix/package.nix
{ lib, stdenv, fetchurl, autoPatchelfHook, glibc }:

let
  version = "0.1.0";

  sources = {
    x86_64-linux = {
      url = "https://github.com/mrtolkien/ghost/releases/download/v${version}/ghost-x86_64-linux";
      hash = "sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
    };
    aarch64-linux = {
      url = "https://github.com/mrtolkien/ghost/releases/download/v${version}/ghost-aarch64-linux";
      hash = "sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
    };
  };

  src = sources.${stdenv.hostPlatform.system}
    or (throw "Unsupported system: ${stdenv.hostPlatform.system}");
in
stdenv.mkDerivation {
  pname = "ghost";
  inherit version;

  src = fetchurl {
    inherit (src) url hash;
  };

  dontUnpack = true;

  nativeBuildInputs = [ autoPatchelfHook ];
  buildInputs = [ glibc ];

  installPhase = ''
    install -Dm755 $src $out/bin/ghost
  '';

  meta = with lib; {
    description = "GHOST personal AI agent platform";
    platforms = [ "x86_64-linux" "aarch64-linux" ];
  };
}
```

Note: placeholder hashes (`sha256-AAA...`) will be updated after the first release.
To get real hashes: `nix-prefetch-url <release-url>`

**Step 2: Create `flake.nix`**

```nix
{
  description = "GHOST personal AI agent platform";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils, ... }:
    flake-utils.lib.eachSystem [ "x86_64-linux" "aarch64-linux" ] (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
      in {
        packages.default = pkgs.callPackage ./nix/package.nix {};
      });
}
```

**Step 3: Generate `flake.lock`**

Run `nix flake lock` to pin nixpkgs. Commit the lock file.

**Step 4: Test locally**

Run: `nix flake check` (will pass even with placeholder hashes — evaluation works,
build would fail until real hashes are set).

**Step 5: Add README section about building from source**

Add a short note about using `rustPlatform.buildRustPackage` for the main branch
(dev/debug only).

**Step 6: Commit**

```
feat: add Nix flake for ghost package
```

---

### Task 4: Default workspace flake — switch to home-manager

Rewrite the workspace flake template to use home-manager instead of `devShells`.

**Files:**
- Modify: `deploy/common/default-flake.nix`

**Step 1: Rewrite the flake template**

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    home-manager = {
      url = "github:nix-community/home-manager";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    ghost.url = "github:mrtolkien/ghost/v0.1.0";
  };

  outputs = { nixpkgs, home-manager, ghost, ... }:
    let
      system = builtins.currentSystem;
      pkgs = nixpkgs.legacyPackages.${system};
    in {
      homeConfigurations.ghost = home-manager.lib.homeManagerConfiguration {
        inherit pkgs;
        modules = [{
          home.username = "root";
          home.homeDirectory = "/root";
          home.stateVersion = "24.11";

          home.packages = with pkgs; [
            # Ghost binary (from flake input)
            ghost.packages.${system}.default

            # Dev tools
            git gh curl wget jq ripgrep fd tree

            # Core POSIX utilities
            coreutils findutils bash gnugrep gnused gawk
            diffutils file less unzip gzip gnutar

            # Python + package manager
            uv python314

            # Database
            sqlite-interactive
          ];

          home.sessionVariables = {
            # Add custom env vars here
          };

          programs.home-manager.enable = true;
        }];
      };
    };
}
```

**Step 2: Commit**

```
feat: switch workspace flake template to home-manager
```

---

### Task 5: Daemon — run `home-manager switch` on boot + remove `nix develop`

Replace `spawn_flake_warmup()` with `home-manager switch` and remove the `nix develop`
wrapping from shell commands. Store the home-manager profile PATH so child processes
inherit it.

**Files:**
- Modify: `src/tools/shell.rs` (replace `spawn_flake_warmup`, simplify `shell_command`)
- Modify: `src/daemon/run.rs:65` (call new boot function)

**Step 1: Write test for home-manager PATH resolution**

```rust
#[test]
fn resolve_hm_profile_finds_bin_dir() {
    let home = TempDir::new().unwrap();
    let profile_bin = home.path().join(".nix-profile/bin");
    std::fs::create_dir_all(&profile_bin).unwrap();
    let result = resolve_hm_profile_path_with_home(home.path());
    assert_eq!(result, Some(profile_bin));
}
```

**Step 2: Run test, verify it fails**

Run: `cargo test -p ghost shell::tests::resolve_hm_profile_finds_bin_dir`

**Step 3: Implement profile PATH resolution + home-manager switch**

Add to `src/tools/shell.rs`:

```rust
use std::sync::OnceLock;

/// PATH prefix from home-manager profile, set after `home-manager switch`.
static HM_PATH_PREFIX: OnceLock<String> = OnceLock::new();

/// Run `home-manager switch` for the workspace flake.
/// Called at daemon boot. GHOST triggers subsequent switches via
/// `run_shell_command`.
pub async fn run_home_manager_switch(workspace: &std::path::Path) -> Result<(), String> {
    let shell_dir = workspace.join("shell");
    if !shell_dir.join("flake.nix").exists() {
        return Ok(());
    }

    tracing::info!("running home-manager switch");
    let output = tokio::process::Command::new("nix")
        .args([
            "run", "home-manager", "--",
            "switch", "--flake",
            shell_dir.to_str().unwrap_or("."),
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .await
        .map_err(|e| format!("failed to run home-manager switch: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("home-manager switch failed: {stderr}"));
    }

    if let Some(profile_bin) = resolve_hm_profile_path() {
        let _ = HM_PATH_PREFIX.set(profile_bin.to_string_lossy().to_string());
        tracing::info!(path = %profile_bin.display(), "home-manager profile PATH set");
    }

    tracing::info!("home-manager switch complete");
    Ok(())
}

fn resolve_hm_profile_path() -> Option<std::path::PathBuf> {
    let home = std::env::var("HOME").ok()?;
    resolve_hm_profile_path_with_home(std::path::Path::new(&home))
}

fn resolve_hm_profile_path_with_home(home: &std::path::Path) -> Option<std::path::PathBuf> {
    let profile_bin = home.join(".nix-profile/bin");
    profile_bin.is_dir().then_some(profile_bin)
}
```

**Step 4: Run test, verify it passes**

**Step 5: Simplify `shell_command()` — remove `nix develop`**

Replace the `shell_command()` function at `src/tools/shell.rs:20-50`:

```rust
fn shell_command(
    command: &str,
    _workspace: &std::path::Path,
    channel_id: Option<&str>,
    session_id: Option<&str>,
) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new("sh");
    cmd.args(["-c", command]);

    // Prepend home-manager profile to PATH
    if let Some(hm_path) = HM_PATH_PREFIX.get() {
        let current_path = std::env::var("PATH").unwrap_or_default();
        cmd.env("PATH", format!("{hm_path}:{current_path}"));
    }

    if let Some(id) = channel_id {
        cmd.env("GHOST_CHANNEL_ID", id);
    }
    if let Some(id) = session_id {
        cmd.env("GHOST_SESSION_ID", id);
    }
    cmd
}
```

**Step 6: Delete `spawn_flake_warmup()`**

Remove lines 210-243 from `src/tools/shell.rs`.

**Step 7: Update daemon boot**

In `src/daemon/run.rs:65`, replace:
```rust
crate::tools::shell::spawn_flake_warmup(config.workspace.clone());
```
with:
```rust
if let Err(e) = crate::tools::shell::run_home_manager_switch(&config.workspace).await {
    logfire::warn!("home-manager switch failed at boot", error = e);
}
```

**Step 8: Run all tests**

Run: `cargo test -p ghost shell::tests`
Run: `just ci`

**Step 9: Commit**

```
feat: replace nix develop wrapping with home-manager profile PATH
```

---

### Task 6: System prompt — parse home-manager packages

Update the flake parser to read `home.packages` instead of `packages = with pkgs;`.

**Files:**
- Modify: `src/prompt/context.rs:31-43` (`parse_flake_packages`)

**Step 1: Update the test**

Modify test at `src/prompt/context.rs:242` to use home-manager format:

```rust
#[test]
fn system_info_includes_shell_tools_from_flake() {
    let dir = TempDir::new().unwrap();
    let shell_dir = dir.path().join("shell");
    fs::create_dir_all(&shell_dir).unwrap();
    fs::write(
        shell_dir.join("flake.nix"),
        "home.packages = with pkgs; [\n  git\n  ripgrep\n  jq\n];\n",
    )
    .unwrap();

    let info = build_system_info(dir.path());
    assert!(info.contains("git, ripgrep, jq"));
}
```

**Step 2: Run test, verify it fails**

Run: `cargo test -p ghost context::tests::system_info_includes_shell_tools_from_flake`

**Step 3: Update `parse_flake_packages()`**

Support both old and new format. Filter out `ghost.packages.*` and comments:

```rust
fn parse_flake_packages(path: &Path) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;

    let marker = if content.contains("home.packages = with pkgs; [") {
        "home.packages = with pkgs; ["
    } else if content.contains("packages = with pkgs; [") {
        "packages = with pkgs; ["
    } else {
        return None;
    };

    let start = content.find(marker)?;
    let after = &content[start..];
    let end = after.find(']')?;
    let block = &after[marker.len()..end];

    let names: Vec<&str> = block
        .split_whitespace()
        .filter(|s| !s.starts_with('#') && !s.starts_with("ghost."))
        .collect();
    if names.is_empty() { return None; }
    Some(names.join(", "))
}
```

**Step 4: Run tests, verify they pass**

Run: `cargo test -p ghost context::tests`

**Step 5: Commit**

```
feat: parse home-manager package list in system prompt
```

---

### Task 7: Dockerfile — simplify

Remove the cargo-chef build stages. Docker image becomes a minimal Nix runtime.

**Files:**
- Modify: `deploy/common/Dockerfile`

**Step 1: Rewrite Dockerfile**

```dockerfile
FROM nixos/nix:latest

# Enable flakes
RUN echo "experimental-features = nix-command flakes" >> /etc/nix/nix.conf

# Copy default flake template and entrypoint
COPY deploy/common/default-flake.nix /opt/ghost/default-flake.nix
COPY deploy/common/entrypoint.sh /opt/ghost/entrypoint.sh

ENV GHOST_CONFIG_DIR=/config
ENV GHOST_WORKSPACE=/workspace

ENTRYPOINT ["/opt/ghost/entrypoint.sh"]
```

**Step 2: Commit**

```
refactor: simplify Dockerfile to minimal Nix runtime
```

---

### Task 8: Entrypoint — bootstrap home-manager on first boot

The entrypoint bootstraps the environment on first boot (when ghost isn't in PATH yet).
After first boot, the `/nix` volume caches everything for fast subsequent starts.

**Files:**
- Modify: `deploy/common/entrypoint.sh`

**Step 1: Rewrite entrypoint**

```sh
#!/bin/sh
set -e

WORKSPACE="${GHOST_WORKSPACE:-/workspace}"

# Ensure workspace shell directory exists with default flake
mkdir -p "$WORKSPACE/shell"
if [ ! -f "$WORKSPACE/shell/flake.nix" ]; then
  cp /opt/ghost/default-flake.nix "$WORKSPACE/shell/flake.nix"
fi

# Bootstrap home-manager environment (installs ghost + all packages).
# Fast if already cached in /nix volume.
echo "Running home-manager switch..."
nix run home-manager -- switch --flake "$WORKSPACE/shell/"

# Source home-manager profile for PATH
export PATH="$HOME/.nix-profile/bin:$PATH"

# Ghost is now in PATH via home-manager
ghost daemon "$@"
```

Note: the entrypoint handles the initial bootstrap. The daemon's own
`run_home_manager_switch()` handles subsequent switches triggered by the GHOST.

**Step 2: Commit**

```
feat: bootstrap home-manager environment in entrypoint
```

---

### Task 9: Skill — update nix-shell.md

Update the GHOST-facing skill to document the home-manager workflow.

**Files:**
- Modify: `prompts/skills/nix-shell.md`

**Step 1: Rewrite the skill**

```markdown
---
name: nix-shell
description:
  Manage the workspace shell environment via Nix + home-manager. Use when you
  need a CLI tool that isn't available, want to install a tool permanently,
  set environment variables, or add shell hooks.
---

# Nix Shell Management

## Workspace flake

`$WORKSPACE/shell/flake.nix` defines the persistent shell environment using
home-manager. All installed packages are available in PATH for every
`run_shell_command`.

## Adding packages permanently

Edit `$WORKSPACE/shell/flake.nix` and add to the `home.packages` list:

    home.packages = with pkgs; [
      # ... existing packages ...
      nodejs
    ];

Then apply the change:

    home-manager switch --flake $WORKSPACE/shell/

Check the output for errors. If the package name is wrong, find it with
`nix search nixpkgs <query>`.

## Setting environment variables

In `$WORKSPACE/shell/flake.nix`, add to `home.sessionVariables`:

    home.sessionVariables = {
      MY_VAR = "value";
    };

Then apply: `home-manager switch --flake $WORKSPACE/shell/`

## Updating flake inputs

To pull the latest version of ghost or nixpkgs:

    nix flake update --flake $WORKSPACE/shell/
    home-manager switch --flake $WORKSPACE/shell/

## One-off tool use

Run a tool without adding it to the flake:

    nix shell nixpkgs#python3 --command python3 -c "print('hello')"
    nix shell nixpkgs#nodePackages.prettier --command prettier --write file.md

## Finding package names

    nix search nixpkgs <query>

Package names in nixpkgs sometimes differ from the command name. Always check
with `nix search` before editing the flake.
```

**Step 2: Commit**

```
docs: update nix-shell skill for home-manager workflow
```

---

### Task 10: Self-update — re-exec on binary change

After shell commands containing `home-manager switch`, check if the ghost binary
changed. If so, trigger graceful re-exec.

**Files:**
- Modify: `src/tools/shell.rs` (add binary hash check)
- Modify: `src/daemon/run.rs` (record boot hash, add re-exec mechanism)

**Step 1: Write test for binary hash comparison**

```rust
#[test]
fn binary_hash_detects_changes() {
    let dir = TempDir::new().unwrap();
    let bin = dir.path().join("ghost");
    std::fs::write(&bin, b"v1").unwrap();
    let h1 = binary_hash(&bin);
    std::fs::write(&bin, b"v2").unwrap();
    let h2 = binary_hash(&bin);
    assert_ne!(h1, h2);
}
```

**Step 2: Run test, verify it fails**

**Step 3: Implement binary hash + re-exec**

Add to `src/tools/shell.rs`:

```rust
use sha2::{Digest, Sha256};

static BOOT_BINARY_HASH: OnceLock<Vec<u8>> = OnceLock::new();

pub fn record_boot_binary_hash() {
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(hash) = binary_hash(&exe_path) {
            let _ = BOOT_BINARY_HASH.set(hash);
        }
    }
}

fn binary_hash(path: &std::path::Path) -> Option<Vec<u8>> {
    let bytes = std::fs::read(path).ok()?;
    Some(Sha256::digest(&bytes).to_vec())
}

pub fn binary_changed() -> bool {
    let Some(boot_hash) = BOOT_BINARY_HASH.get() else {
        return false;
    };
    let Ok(exe_path) = std::env::current_exe() else {
        return false;
    };
    binary_hash(&exe_path).is_some_and(|h| &h != boot_hash)
}
```

In `src/daemon/run.rs`, add near line 65:
```rust
crate::tools::shell::record_boot_binary_hash();
```

For re-exec, use `std::os::unix::process::CommandExt`:
```rust
pub fn re_exec() -> ! {
    let exe = std::env::current_exe().expect("cannot resolve exe path");
    let args: Vec<String> = std::env::args().collect();
    tracing::info!("re-executing ghost with updated binary");
    let err = std::process::Command::new(&exe).args(&args[1..]).exec();
    panic!("re-exec failed: {err}");
}
```

The trigger: after `run_shell_command` returns for commands containing
`home-manager switch`, call `binary_changed()`. If true, initiate graceful
shutdown and re-exec. Exact trigger mechanism to be refined during implementation.

**Step 4: Run test, verify it passes**

**Step 5: Run full tests**

Run: `just ci`

**Step 6: Commit**

```
feat: detect ghost binary changes and support re-exec for self-update
```

---

### Task 11: Verify workspace bootstrap

Confirm `include_str!` at `src/config_workspace.rs:8` still works with the updated
`deploy/common/default-flake.nix`.

**Files:**
- Verify: `src/config_workspace.rs:8`

**Step 1: Run cargo check**

Run: `cargo check`

The `include_str!("../deploy/common/default-flake.nix")` path is unchanged.

**Step 2: Commit if needed**

---

### Task 12: Final verification

**Step 1: Run full CI**

Run: `just ci`

**Step 2: Build Docker image locally**

Run: `docker build -f deploy/common/Dockerfile -t ghost:test .`

Verify image is ~150 MB (vs previous ~1 GB+).

**Step 3: Test first boot flow**

```sh
docker run --rm -v ghost-test-nix:/nix -v /tmp/ghost-workspace:/workspace \
  -e GHOST_CONFIG_DIR=/config ghost:test
```

Verify:
- Entrypoint copies default flake to workspace
- `home-manager switch` runs and installs packages + ghost
- Ghost daemon starts
- Shell commands work without `nix develop` wrapping

**Step 4: Test flake edit flow**

Edit `/workspace/shell/flake.nix` (add a package), run
`home-manager switch --flake /workspace/shell/`, verify the new package is available.

**Step 5: Commit any final fixes**

```
chore: final verification of nix shell improvements
```
