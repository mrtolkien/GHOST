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

    home-manager switch --flake $WORKSPACE/shell/#ghost

Check the output for errors. If the package name is wrong, find it with
`nix search nixpkgs <query>`.

## Setting environment variables

In `$WORKSPACE/shell/flake.nix`, add to `home.sessionVariables`:

    home.sessionVariables = {
      MY_VAR = "value";
    };

Then apply: `home-manager switch --flake $WORKSPACE/shell/#ghost`

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
