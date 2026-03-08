# Nix Shell Improvements Design

## Problem

1. Every `run_shell_command` wraps in `nix develop` (~0.5s overhead per command)
2. Ghost binary is baked into Docker image — updating requires image rebuild + container restart
3. GHOST can't manage env vars or shell hooks declaratively

## Solution

Replace `nix develop` wrapping with **home-manager** as the system shell manager. Ghost
binary becomes a Nix flake input fetched from GitHub releases.

## Architecture

### CI: Binary Releases

Current flow builds the binary inside Docker. New flow (3 jobs):

1. **`build-binary`** — native runners (x64 + arm), `cargo build --release && strip`,
   upload binary as workflow artifact
2. **`release`** — on `v*` tags: attach binaries to GitHub Release
3. **`docker`** — build minimal image (no binary build stages, no ghost binary)

### Ghost Nix Flake (this repo)

Top-level `flake.nix` defines a package that fetches the pre-built ghost binary from
GitHub releases by content hash, per architecture. Handles `patchelf` for glibc
compatibility (currently done in Dockerfile).

For dev/debug: users can override the flake input to `github:mrtolkien/ghost/main` and
build from source via `rustPlatform.buildRustPackage` (documented in README, not a
default path).

### Workspace Flake: home-manager

The default `shell/flake.nix` template switches from `devShells.default` (mkShell) to a
home-manager configuration:

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    home-manager.url = "github:nix-community/home-manager";
    ghost.url = "github:mrtolkien/ghost/v0.1.0";
  };

  outputs = { nixpkgs, home-manager, ghost, ... }:
    let
      system = "x86_64-linux"; # or detect
      pkgs = nixpkgs.legacyPackages.${system};
    in {
      homeConfigurations.ghost = home-manager.lib.homeManagerConfiguration {
        inherit pkgs;
        modules = [{
          home.username = "root";
          home.homeDirectory = "/root";
          home.stateVersion = "24.11";
          home.packages = with pkgs; [
            git gh curl wget jq ripgrep fd tree
            coreutils findutils bash gnugrep gnused gawk
            diffutils file less unzip gzip gnutar uv python314
            sqlite-interactive
            ghost.packages.${system}.default
          ];
          home.sessionVariables = { };
          programs.home-manager.enable = true;
        }];
      };
    };
}
```

GHOST edits this file to add/remove packages, set env vars, and add shell hooks.

### Daemon Behavior

**On boot:**
- Run `home-manager switch --flake $WORKSPACE/shell/` to ensure environment is set up
- This replaces `spawn_flake_warmup()` — first boot downloads everything, subsequent
  boots are near-instant (nix store cached in `/nix` volume)

**Shell commands:**
- No more `nix develop` wrapping. `Command::new("sh")` directly — packages are in PATH
  via home-manager's profile
- Daemon sets PATH from home-manager profile on spawned commands

**No file watcher for flake.nix** — the GHOST runs `home-manager switch` explicitly via
`run_shell_command` (guided by the nix-shell skill) so it sees errors and can fix them.

**Self-update / re-exec:**
- After any `run_shell_command` completes, daemon checks ghost binary hash (cheap stat)
- If binary changed (new version installed via `home-manager switch`), graceful re-exec
  with same args
- Sessions are DB-backed, nothing lost on re-exec

### Docker Image

Becomes minimal — no build stages, no ghost binary:

```dockerfile
FROM nixos/nix:latest
RUN echo "experimental-features = nix-command flakes" >> /etc/nix/nix.conf
COPY deploy/common/entrypoint.sh /opt/ghost/entrypoint.sh
COPY deploy/common/default-flake.nix /opt/ghost/default-flake.nix
ENTRYPOINT ["/opt/ghost/entrypoint.sh"]
```

First boot of a fresh deployment is slower (~30-60s downloading packages from nix cache).
`/nix` docker volume caches the nix store across container restarts. Image upgrades no
longer needed for ghost updates — just `nix flake update && home-manager switch` inside
the container.

### Skill Update

`prompts/skills/nix-shell.md` updated to guide the GHOST through home-manager commands:

- Editing `shell/flake.nix` for packages, env vars, shell hooks
- Running `home-manager switch --flake $WORKSPACE/shell/` to apply
- Running `nix flake update --flake $WORKSPACE/shell/` for input updates
- Running `nix search nixpkgs <query>` to find packages
- One-off tool use with `nix shell nixpkgs#<pkg> --command ...`

### System Prompt

`parse_flake_packages()` updated to read home-manager's `home.packages` list instead of
`devShells.default.packages`.

## Migration

This is a breaking change to the workspace flake format. Per CLAUDE.md: pre-alpha, no
backwards compatibility needed. The old `devShells`-based `shell/flake.nix` is replaced
on workspace bootstrap.

## Key Decisions

- **home-manager over nix profile**: marginal complexity cost, but gives declarative
  env vars and shell hooks — a richer surface for the GHOST to manage
- **Ghost binary via flake input**: self-update without image rebuild, version pinned
  in flake (reproducible)
- **No file watcher**: GHOST runs `home-manager switch` explicitly, sees errors
- **Re-exec for self-update**: simpler than systemd, works on nixos/nix base image
- **Build from source for main branch**: documented escape hatch for dev/debug, not
  a default path (avoids CI hash-commit machinery)
