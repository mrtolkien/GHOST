---
name: nix-shell
description:
  Manage the workspace shell environment via Nix. Use when you need a CLI tool that
  isn't available, want to install a tool permanently, or manage the shell environment.
---

# Nix Shell Management

## Workspace flake

`$WORKSPACE/shell/flake.nix` defines the shell environment using `nix develop`. All
installed packages are available in PATH for every `run_shell_command`.

## Adding packages permanently

Edit `$WORKSPACE/shell/flake.nix` and add to the `packages` list:

    packages = with pkgs; [
      # ... existing packages ...
      nodejs
    ];

Then **validate the change** by running:

    nix develop $WORKSPACE/shell --command sh -c "echo ok"

Check the output for errors. If the package name is wrong, find it with
`nix search nixpkgs <query>`.

**Important**: After validating, new packages take effect on the next daemon restart.
Tell the OPERATOR that a restart is needed to pick up changes.

## Updating flake inputs

To pull the latest nixpkgs:

    nix flake update --flake $WORKSPACE/shell/
    nix develop $WORKSPACE/shell --command sh -c "echo ok"

## One-off tool use

Run a tool without adding it to the flake:

    nix shell nixpkgs#python3 --command python3 -c "print('hello')"
    nix shell nixpkgs#nodePackages.prettier --command prettier --write file.md

## Finding package names

    nix search nixpkgs <query>

Package names in nixpkgs sometimes differ from the command name. Always check with
`nix search` before editing the flake.
