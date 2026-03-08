# Nix Shell Improvements Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace per-command `nix develop` wrapping with home-manager as the system
shell manager, and make the ghost binary a Nix flake input for self-update without image
rebuilds.

**Architecture:** CI publishes release binaries to GitHub Releases. A top-level
`flake.nix` defines a ghost package that fetches those binaries by hash. The workspace
`shell/flake.nix` uses home-manager with ghost as an input. The entrypoint runs
`home-manager switch` to bootstrap (installing ghost + packages). The daemon runs
`home-manager switch` on boot for subsequent updates. Shell commands run without
`nix develop` wrapping — packages are in PATH via home-manager's profile.

**Tech Stack:** Nix flakes, home-manager (standalone), GitHub Actions

**Design doc:** `docs/plans/2026-03-08-nix-shell-improvements-design.md`

---

## Dependency diagram

```
Phase 1 (CI + Nix flake — needs a real release before Phase 2):
  Task 1: CI binary build job
  Task 2: CI release upload job (depends on Task 1)
  Task 3: Ghost Nix flake with placeholder hashes

  >>> STOP: push workflow, create v0.1.0 tag, get real binary hashes <<<

Phase 2 (Nix flake hashes + workspace flake):
  Task 4: Update nix/package.nix with real hashes
  Task 5: Workspace flake template → home-manager

Phase 3 (Rust daemon changes — all independent of each other):
  Task 6: Daemon — home-manager switch on boot + remove nix develop
  Task 7: System prompt — parse home-manager packages
  Task 8: Skill — update nix-shell.md

Phase 4 (Docker — depends on Phase 2 + 3):
  Task 9: Dockerfile — simplify (no build stages, no ghost binary)
  Task 10: Entrypoint — bootstrap home-manager on first boot

Phase 5 (Self-update — can be done later):
  Task 11: Re-exec on binary change
```

**IMPORTANT — what the Docker image does NOT contain:**
The Docker image has NO ghost binary. It is a minimal `nixos/nix` image with the
default flake template and entrypoint script. On first boot, the entrypoint runs
`home-manager switch` which fetches the ghost binary from GitHub Releases via the
Nix flake. The `/nix` Docker volume caches everything for subsequent boots.

---

### Task 1: CI — Add binary build job

Add a standalone CI job that builds the ghost binary on native runners and uploads
it as a workflow artifact. This is separate from the Docker job — the Docker job
does NOT use this artifact. The artifact is only used by the release upload job
(Task 2).

**Files:**
- Modify: `.github/workflows/docker.yml`

**Step 1: Add `build-binary` job**

Add this job BEFORE the existing `build` job. It runs in parallel with Docker —
no dependency between them.

```yaml
  build-binary:
    strategy:
      matrix:
        include:
          - runner: ubuntu-latest
            target: x86_64-unknown-linux-gnu
            artifact: ghost-x86_64-linux
          - runner: ubuntu-24.04-arm
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
        run: |
          cargo build --release --target ${{ matrix.target }}
          strip target/${{ matrix.target }}/release/ghost

      - uses: actions/upload-artifact@v4
        with:
          name: ${{ matrix.artifact }}
          path: target/${{ matrix.target }}/release/ghost
          retention-days: 7
```

The existing `build` (Docker) and `merge` jobs stay UNCHANGED. `build-binary` runs
in parallel with them.

**Step 2: Commit**

```
feat: add standalone binary build CI job
```

---

### Task 2: CI — Add release upload job

On `v*` tags, download the binary artifacts and attach them to the GitHub Release.
These URLs are what the Nix flake fetches.

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

**Step 2: Commit**

```
feat: upload release binaries to GitHub Releases on tags
```

---

### Task 3: Ghost Nix flake with placeholder hashes

Create the top-level `flake.nix` and `nix/package.nix` that define the ghost
package. Use placeholder hashes — they'll be filled in after the first release.

**Files:**
- Create: `nix/package.nix`
- Create: `flake.nix` (repo root)

**Step 1: Create `nix/package.nix`**

```nix
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

Run: `nix flake lock`

Commit the lock file.

**Step 4: Commit**

```
feat: add Nix flake for ghost package (placeholder hashes)
```

---

### >>> STOP HERE <<<

**Push Tasks 1-3 to a branch and merge to main. Then:**

1. Create a `v0.1.0` tag on main
2. Wait for CI to run — `build-binary` builds the binaries, `release` attaches them
   to the GitHub Release
3. Get the real sha256 hashes:
   ```sh
   nix-prefetch-url https://github.com/mrtolkien/ghost/releases/download/v0.1.0/ghost-x86_64-linux
   nix-prefetch-url https://github.com/mrtolkien/ghost/releases/download/v0.1.0/ghost-aarch64-linux
   ```
4. Resume with Task 4

---

### Task 4: Update Nix flake with real hashes

Replace the placeholder hashes in `nix/package.nix` with the real ones from the
v0.1.0 release.

**Files:**
- Modify: `nix/package.nix`

**Step 1: Replace hashes**

Replace the `sha256-AAA...` placeholders with the real hashes obtained from
`nix-prefetch-url`.

**Step 2: Verify the flake builds**

Run: `nix build .#default`

This should fetch the binary from GitHub Releases, patch it with autoPatchelfHook,
and produce `result/bin/ghost`.

**Step 3: Commit**

```
feat: set real binary hashes in Nix flake for v0.1.0
```

---

### Task 5: Workspace flake template — switch to home-manager

Rewrite the default workspace flake to use home-manager instead of `devShells`.

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

**Step 2: Verify `include_str!` still works**

Run: `cargo check`

The existing `include_str!("../deploy/common/default-flake.nix")` in
`src/config_workspace.rs:8` points to the same file — just verify it compiles.

**Step 3: Commit**

```
feat: switch workspace flake template to home-manager
```

---

### Task 6: Daemon — home-manager switch on boot + remove `nix develop`

Replace `spawn_flake_warmup()` with `run_home_manager_switch()`. Remove the
`nix develop` wrapping from `shell_command()`. Store the home-manager profile
PATH so child processes inherit it.

**Files:**
- Modify: `src/tools/shell.rs`
- Modify: `src/daemon/run.rs:65`

**Step 1: Write test for profile PATH resolution**

Add to `src/tools/shell.rs` tests:

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

**Step 3: Add profile PATH resolution + home-manager switch function**

Add to `src/tools/shell.rs`:

```rust
use std::sync::OnceLock;

/// PATH prefix from home-manager profile.
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

**Step 5: Simplify `shell_command()` — remove `nix develop` wrapping**

Replace the entire `shell_command()` function at `src/tools/shell.rs:20-50`.
The new version does NOT check for `shell/flake.nix` and does NOT use `nix develop`:

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

Delete the entire function at `src/tools/shell.rs:212-243`.

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

### Task 7: System prompt — parse home-manager packages

Update the flake parser to read `home.packages` instead of `packages = with pkgs;`.

**Files:**
- Modify: `src/prompt/context.rs` (`parse_flake_packages` function)

**Step 1: Update the test**

Modify `system_info_includes_shell_tools_from_flake` test to use the new format:

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
    if names.is_empty() {
        return None;
    }
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

### Task 8: Skill — update nix-shell.md

Update the GHOST-facing skill for the home-manager workflow.

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

### Task 9: Dockerfile — simplify

Remove ALL build stages. The Docker image has NO ghost binary. It is a minimal
`nixos/nix` image that only contains the default flake template and entrypoint.

**IMPORTANT: The Docker image does NOT download, build, or include the ghost binary
in any way. Ghost is installed at runtime by home-manager via the Nix flake.**

**Files:**
- Modify: `deploy/common/Dockerfile`

**Step 1: Rewrite Dockerfile**

Delete the entire file and replace with:

```dockerfile
FROM nixos/nix:latest

# Enable flakes
RUN echo "experimental-features = nix-command flakes" >> /etc/nix/nix.conf

# Copy default flake template and entrypoint
COPY deploy/common/default-flake.nix /opt/ghost/default-flake.nix
COPY deploy/common/entrypoint.sh /opt/ghost/entrypoint.sh
RUN chmod +x /opt/ghost/entrypoint.sh

ENV GHOST_CONFIG_DIR=/config
ENV GHOST_WORKSPACE=/workspace

ENTRYPOINT ["/opt/ghost/entrypoint.sh"]
```

That's it. No `cargo-chef`. No `COPY --from=builder`. No `patchelf`. No ghost binary.

**Step 2: Commit**

```
refactor: simplify Dockerfile to minimal Nix runtime (no ghost binary)
```

---

### Task 10: Entrypoint — bootstrap home-manager on first boot

The entrypoint runs `home-manager switch` before starting the daemon. This is the
mechanism that installs the ghost binary on first boot. The `/nix` Docker volume
caches everything for fast subsequent starts.

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
# First boot downloads everything (~30-60s). Subsequent boots are fast
# because the /nix volume caches the store.
echo "Running home-manager switch..."
nix run home-manager -- switch --flake "$WORKSPACE/shell/"

# Add home-manager profile to PATH so we can find ghost
export PATH="$HOME/.nix-profile/bin:$PATH"

# Ghost is now available via home-manager — start the daemon
ghost daemon "$@"
```

**Step 2: Commit**

```
feat: bootstrap home-manager environment in entrypoint
```

---

### Task 11: Self-update — re-exec on binary change (optional, can defer)

After shell commands containing `home-manager switch`, check if the ghost binary
changed. If so, trigger graceful re-exec.

**Files:**
- Modify: `src/tools/shell.rs` (add binary hash check)
- Modify: `src/daemon/run.rs` (record boot hash, add re-exec)

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

For re-exec, use `std::os::unix::process::CommandExt::exec()`:
```rust
pub fn re_exec() -> ! {
    use std::os::unix::process::CommandExt;
    let exe = std::env::current_exe().expect("cannot resolve exe path");
    let args: Vec<String> = std::env::args().collect();
    tracing::info!("re-executing ghost with updated binary");
    let err = std::process::Command::new(&exe).args(&args[1..]).exec();
    panic!("re-exec failed: {err}");
}
```

Trigger: after `run_shell_command` returns for commands containing
`home-manager switch`, call `binary_changed()`. If true, initiate graceful
shutdown and re-exec.

**Step 4: Run test, verify it passes**

**Step 5: Run full tests**

Run: `just ci`

**Step 6: Commit**

```
feat: detect ghost binary changes and support re-exec for self-update
```

---

### Task 12: Final verification

**Step 1: Run full CI**

Run: `just ci`

**Step 2: Build Docker image locally**

Run: `docker build -f deploy/common/Dockerfile -t ghost:test .`

Verify image is small (~150 MB — just `nixos/nix` base + 2 files).

**Step 3: Test first boot flow**

```sh
docker run --rm -v ghost-test-nix:/nix -v /tmp/ghost-workspace:/workspace \
  -e GHOST_CONFIG_DIR=/config ghost:test
```

Verify:
- Entrypoint copies default flake to `/workspace/shell/flake.nix`
- `home-manager switch` runs and installs packages + ghost binary
- Ghost daemon starts successfully
- Shell commands work without `nix develop` wrapping

**Step 4: Test flake edit flow**

Edit `/workspace/shell/flake.nix` (add a package), run
`home-manager switch --flake /workspace/shell/`, verify the new package is available
in subsequent `run_shell_command` calls.
