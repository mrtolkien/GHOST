---
name: nix-shell
description:
  Manage the workspace shell environment via Nix. Use when you need a CLI tool that
  isn't available, want to install a tool permanently, or need to run a one-off command
  with a specific package.
---

# Nix Shell Management

## Workspace flake

`$WORKSPACE/shell/flake.nix` defines the persistent shell environment. Every
`run_shell_command` runs inside `nix develop $WORKSPACE/shell/ --command ...`, so
changes to the flake take effect on the next command — no restart needed.

## One-off tool use

Run a tool without adding it to the flake:

```sh
nix shell nixpkgs#python3 --command python3 -c "print('hello')"
nix shell nixpkgs#nodePackages.prettier --command prettier --write file.md
```

## Permanent install

Edit `$WORKSPACE/shell/flake.nix` and add the package to `packages`:

```nix
packages = with pkgs; [
  # ... existing packages ...
  python3
];
```

The next `run_shell_command` will pick it up automatically.

## Finding package names

```sh
nix search nixpkgs <query>
```

Package names in nixpkgs sometimes differ from the command name. Check with `nix search`
before editing the flake.
