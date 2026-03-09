# Nix Shell Improvements Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this
> plan task-by-task.

**Goal:** Replace per-command `nix develop` wrapping with home-manager as the system
shell manager. Ghost binary stays Docker-built — no changes to CI or Dockerfile build.

**Architecture:** The workspace `shell/flake.nix` uses home-manager (without ghost as a
flake input). The daemon runs `home-manager switch` on boot and the GHOST runs it via
shell commands after editing the flake. Shell commands run without `nix develop`
wrapping — packages are in PATH via home-manager's profile.

**Tech Stack:** Nix flakes, home-manager (standalone)

**Design doc:** `docs/plans/2026-03-08-nix-shell-improvements-design.md`

---

### Task 1: Default workspace flake — switch to home-manager

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
  };

  outputs = { nixpkgs, home-manager, ... }:
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

### Task 2: Daemon — run `home-manager switch` on boot + remove `nix develop`

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
    let output = tokio::process::Command::new("home-manager")
        .args([
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

Remove the function from `src/tools/shell.rs`.

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

Run: `cargo test -p ghost shell::tests` Run: `just ci`

**Step 9: Commit**

```
feat: replace nix develop wrapping with home-manager profile PATH
```

---

### Task 3: System prompt — parse home-manager packages

Update the flake parser to read `home.packages` instead of `packages = with pkgs;`.

**Files:**

- Modify: `src/prompt/context.rs` (`parse_flake_packages`)

**Step 1: Update the test**

Modify the test to use home-manager format:

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

Run: `cargo test context::tests::system_info_includes_shell_tools_from_flake`

**Step 3: Update `parse_flake_packages()`**

Support both old and new format. Filter out comments:

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
        .filter(|s| !s.starts_with('#'))
        .collect();
    if names.is_empty() { return None; }
    Some(names.join(", "))
}
```

**Step 4: Run tests, verify they pass**

Run: `cargo test context::tests`

**Step 5: Commit**

```
feat: parse home-manager package list in system prompt
```

---

### Task 4: Skill — update nix-shell.md

Update the GHOST-facing skill to document the home-manager workflow, including where to
find documentation.

**Files:**

- Modify: `prompts/skills/nix-shell.md`

**Step 1: Rewrite the skill**

```markdown
---
name: nix-shell
description:
  Manage the workspace shell environment via Nix + home-manager. Use when you need a CLI
  tool that isn't available, want to install a tool permanently, set environment
  variables, or add shell hooks.
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

To pull the latest nixpkgs:

    nix flake update --flake $WORKSPACE/shell/
    home-manager switch --flake $WORKSPACE/shell/

## One-off tool use

Run a tool without adding it to the flake:

    nix shell nixpkgs#python3 --command python3 -c "print('hello')"
    nix shell nixpkgs#nodePackages.prettier --command prettier --write file.md

## Finding package names

    nix search nixpkgs <query>

Package names in nixpkgs sometimes differ from the command name. Always check with
`nix search` before editing the flake.

## Documentation

If you need home-manager docs (available options, programs.\* modules, etc.), use the
`context7` MCP to look up `home-manager` documentation. For nixpkgs package search, use
`nix search nixpkgs <query>`.
```

**Step 2: Commit**

```
docs: update nix-shell skill for home-manager workflow
```

---

### Task 5: Entrypoint — run home-manager switch on boot

Update the Docker entrypoint to run `home-manager switch` before starting the daemon.
This ensures the nix environment is ready. The ghost binary is already in the image.

**Files:**

- Modify: `deploy/common/entrypoint.sh`

**Step 1: Update entrypoint**

Add home-manager switch before `ghost daemon`:

```sh
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
nix run home-manager -- switch --flake "$WORKSPACE/shell/"

# Source home-manager profile for PATH
export PATH="$HOME/.nix-profile/bin:$PATH"

exec /usr/local/bin/ghost daemon "$@"
```

Note: ghost binary is at `/usr/local/bin/ghost` from the Docker build, not from
home-manager. The `home-manager switch` only sets up the workspace toolchain.

**Step 2: Commit**

```
feat: run home-manager switch in entrypoint for workspace toolchain
```

---

### Task 6: Final verification

**Step 1: Run full CI**

Run: `just ci`

**Step 2: Build Docker image locally**

Run: `docker build -f deploy/common/Dockerfile -t ghost:test .`

**Step 3: Test first boot flow**

```sh
docker run --rm -v ghost-test-nix:/nix -v /tmp/ghost-workspace:/workspace \
  -e GHOST_CONFIG_DIR=/config ghost:test
```

Verify:

- Entrypoint copies default flake to workspace
- `home-manager switch` runs and installs packages
- Ghost daemon starts
- Shell commands work without `nix develop` wrapping

**Step 4: Test flake edit flow**

Edit `/workspace/shell/flake.nix` (add a package), run
`home-manager switch --flake /workspace/shell/`, verify the new package is available.

**Step 5: Commit any final fixes**

```
chore: final verification of nix shell improvements
```
