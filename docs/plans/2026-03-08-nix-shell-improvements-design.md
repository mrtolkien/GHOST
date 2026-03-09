# Nix Shell Improvements Design

## Problem

1. Every `run_shell_command` wraps in `nix develop` (~0.5s overhead per command)
2. GHOST can't manage env vars or shell hooks declaratively

## Solution

Replace `nix develop` wrapping with **home-manager** as the system shell manager.
Ghost binary stays Docker-built — updates via new image pulls.

## Out of Scope (for now)

- CI binary releases / GitHub Release artifacts
- Ghost Nix flake (fetching binary from releases)
- Self-update / re-exec mechanism
- Dockerfile simplification (keep current multi-stage build with ghost binary baked in)

These will be revisited once we're happy with the project state and want clean
nix-installable binary releases.

## Architecture

### Workspace Flake: home-manager

The default `shell/flake.nix` template switches from `devShells.default` (mkShell) to a
home-manager configuration:

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
            git gh curl wget jq ripgrep fd tree
            coreutils findutils bash gnugrep gnused gawk
            diffutils file less unzip gzip gnutar uv python314
            sqlite-interactive
          ];

          home.sessionVariables = { };

          programs.home-manager.enable = true;
        }];
      };
    };
}
```

GHOST edits this file to add/remove packages, set env vars, and add shell hooks.
Ghost binary is NOT in the flake — it's baked into the Docker image.

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

### Docker Image

Keeps the current multi-stage build. Ghost binary is still built and baked into the
image. The only change is that the entrypoint runs `home-manager switch` on boot
(after the ghost binary is already available in the image).

First boot of a fresh deployment is slower (~30-60s downloading packages from nix cache).
`/nix` docker volume caches the nix store across container restarts.

### Skill Update

`prompts/skills/nix-shell.md` updated to guide the GHOST through home-manager commands:

- Editing `shell/flake.nix` for packages, env vars, shell hooks
- Running `home-manager switch --flake $WORKSPACE/shell/` to apply
- Running `nix flake update --flake $WORKSPACE/shell/` for input updates
- Running `nix search nixpkgs <query>` to find packages
- One-off tool use with `nix shell nixpkgs#<pkg> --command ...`
- Where to find home-manager docs (context7 or NixOS wiki)

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
- **Ghost binary stays in Docker**: simplicity — no flake-based binary distribution,
  no self-update mechanism, no CI release pipeline changes needed
- **No file watcher**: GHOST runs `home-manager switch` explicitly, sees errors
- **Binary releases deferred**: will revisit when project stabilizes
