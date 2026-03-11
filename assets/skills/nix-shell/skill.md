---
name: nix-shell
description:
  Manage the workspace shell environment via Nix. Use when you need a CLI tool that
  isn't available, want to install a tool permanently, or manage the shell environment.
---

# Nix Shell Management

## Workspace flake

`$WORKSPACE/shell/flake.nix` defines the shell environment as a `buildEnv` package. At
daemon boot, `nix build` creates a merged store path whose `bin/` is prepended to PATH
for every `run_shell_command`.

## Adding packages permanently

Edit `$WORKSPACE/shell/flake.nix` and add to the `paths` list:

    paths = with pkgs; [
      # ... existing packages ...
      nodejs
    ];

Then **validate the change** by running:

    nix build $WORKSPACE/shell --no-link

Check the output for errors. If the package name is wrong, find it with
`nix search nixpkgs <query>`.

**Important**: After validating, new packages take effect on the next daemon restart.
Tell the OPERATOR that a restart is needed to pick up changes.

## Updating flake inputs

To pull the latest nixpkgs (and ghost binary if using pre-built releases):

    nix flake update --flake $WORKSPACE/shell/
    nix build $WORKSPACE/shell --no-link

## Self-updating ghost

The live flake can include the ghost binary itself. When `nix flake update` pulls a
newer version, the daemon must restart to use it. To trigger a restart:

    nix flake update --flake $WORKSPACE/shell/
    nix build $WORKSPACE/shell --no-link
    kill 1

Ghost is PID 1 in the container. `kill 1` sends SIGTERM, Docker restarts the container,
and the entrypoint runs `nix build` against the updated live flake — picking up the new
ghost binary.

Tell the OPERATOR before running `kill 1` — there will be a brief downtime while the
container restarts.

## One-off tool use

Run a tool without adding it to the flake:

    nix shell nixpkgs#python3 --command python3 -c "print('hello')"
    nix shell nixpkgs#nodePackages.prettier --command prettier --write file.md

## Finding package names

    nix search nixpkgs <query>

Package names in nixpkgs sometimes differ from the command name. Always check with
`nix search` before editing the flake.
