# Nix Environment

## Overview

Nix is the full lifecycle tool for GHOST: install a pre-built binary, run it as a
service, and let the GHOST manage its own shell tools. The OPERATOR never compiles
anything.

## The OPERATOR Experience

```bash
# 1. Install (pre-built binary, no compilation)
nix profile install nixpkgs#ghost

# 2. First-time setup
ghost init

# 3. Register as a background service
ghost daemon install

# 4. Done — GHOST is running
ghost daemon status
```

## Distribution: Pre-Built Binaries

### CI Pipeline

GitHub Actions builds release binaries for each target on every tag:

- `x86_64-linux`
- `aarch64-linux`
- `x86_64-darwin`
- `aarch64-darwin` (Apple Silicon)

Binaries are uploaded as GitHub release assets. This is the source of truth for
distribution — everything else pulls from here.

### nixpkgs Package (ideal)

Submit a package to nixpkgs that fetches the pre-built binary from GitHub releases.
Pattern: `fetchurl` + `autoPatchelfHook` (Linux) or `installPhase` (macOS).

```nix
# Simplified — real package would have per-system URLs and hashes
{ lib, stdenv, fetchurl, autoPatchelfHook }:
stdenv.mkDerivation rec {
  pname = "ghost";
  version = "0.1.0";

  src = fetchurl {
    url = "https://github.com/user/ghost/releases/download/v${version}/ghost-${stdenv.hostPlatform.system}";
    hash = "sha256-...";
  };

  nativeBuildInputs = lib.optionals stdenv.isLinux [ autoPatchelfHook ];

  installPhase = ''
    install -Dm755 $src $out/bin/ghost
  '';
}
```

Once in nixpkgs: `nix profile install nixpkgs#ghost` — pulls the binary, no build.

### Project Flake (for development and early adopters)

Before the nixpkgs package is accepted, the repo ships a `flake.nix` using `crane` so
early adopters can install directly:

```bash
nix profile install github:user/ghost
```

This builds from source (slow first time, cached after). The flake also provides the
`devShell` for contributors.

## Service Registration: `ghost daemon install`

The `ghost` binary itself handles service registration. No home-manager, no NixOS
module, no manual unit files. Works on any machine where the binary runs.

```bash
ghost daemon install    # Register as a background service
ghost daemon uninstall  # Remove the service
ghost daemon status     # Check if running
ghost daemon logs       # Tail service logs
```

### Linux (systemd user service)

`ghost daemon install` writes a systemd user unit:

```
~/.config/systemd/user/ghost.service
```

```ini
[Unit]
Description=GHOST AI Agent

[Service]
ExecStart=/path/to/ghost daemon
Restart=on-failure
RestartSec=5
Environment=GHOST_WORKSPACE=%h/GHOST
Environment=GHOST_CONFIG_DIR=%h/.config/ghost
EnvironmentFile=-%h/.config/ghost/.env

[Install]
WantedBy=default.target
```

Then runs `systemctl --user daemon-reload && systemctl --user enable --now ghost`.

No root required. Works on any Linux with systemd (NixOS, Ubuntu, Arch, etc.).

### macOS (launchd)

`ghost daemon install` writes a launchd plist:

```
~/Library/LaunchAgents/com.ghost.daemon.plist
```

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "...">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.ghost.daemon</string>
    <key>ProgramArguments</key>
    <array>
        <string>/path/to/ghost</string>
        <string>daemon</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>/tmp/ghost.stdout.log</string>
    <key>StandardErrorPath</key>
    <string>/tmp/ghost.stderr.log</string>
</dict>
</plist>
```

Then runs `launchctl load`. Works on any Mac — no Homebrew services, no home-manager.

### Optional: NixOS / home-manager Modules

For users who prefer declarative config, the project flake can also export NixOS and
home-manager modules. These are convenience wrappers — not the primary path.

## GHOST's Shell: Nix-Managed Tool Environment

Once GHOST is running, it uses Nix to manage its own CLI tools. This is independent of
how the OPERATOR installed GHOST.

### Workspace Flake

`ghost init` creates a `flake.nix` in the workspace with default tool dependencies:

```nix
{
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

  outputs = { nixpkgs, ... }:
    let pkgs = nixpkgs.legacyPackages.x86_64-linux;
    in {
      devShells.default = pkgs.mkShell {
        packages = with pkgs; [
          ripgrep
          fd
          jq
          curl
          # GHOST can add more here via file_edit
        ];
      };
    };
}
```

The daemon wraps `run_shell_command` in `nix develop` so every command runs with these
tools available.

### On-Demand Tool Installation

The GHOST can use tools without editing the flake:

```bash
nix shell nixpkgs#python3 -- python3 script.py
nix shell nixpkgs#imagemagick -- convert input.png output.jpg
```

Safe (Nix store is immutable), no OPERATOR approval needed.

### Self-Extending

The GHOST permanently adds tools by editing its workspace `flake.nix` via `file_edit`. A
skill guides this: search nixpkgs for the package name, add it, next command picks it
up.

## Prerequisites

- Nix must be installed on the host (with flakes enabled)
- The daemon detects Nix availability at startup and falls back to raw bash if absent
- Service registration (`ghost daemon install`) works with or without Nix — it just
  needs the binary on `PATH`
- GHOST's shell management (workspace flake) requires Nix and degrades gracefully

## Graceful Degradation

Users without Nix can still use GHOST:

- Build from source with `cargo build --release`
- Run `ghost daemon install` for service registration (still works)
- `run_shell_command` uses whatever tools are on the system `PATH`
- The workspace flake is simply not created by `ghost init`
