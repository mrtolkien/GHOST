# Auto-install container runtime during onboarding

If `ghost init` detects no container runtime (podman or docker), offer to install podman
rootless via nix and configure it.

## Why

Several services (SearXNG, Crawl4AI, Docling) run as containers. Without a container
runtime, those options are unavailable and the wizard defaults to skipping them. Setting
up podman automatically would make the full service stack accessible out of the box.

## Install steps

1. `nix profile add nixpkgs#podman nixpkgs#podman-compose`
2. Platform-specific post-install:
   - **Debian/Ubuntu**: check for `uidmap` package (provides setuid `newuidmap`/
     `newgidmap`). If missing, tell user to `sudo apt install uidmap`. Also verify
     `/etc/subuid` + `/etc/subgid` entries exist for the user.
   - **macOS**: run `podman machine init && podman machine start` (uses Apple
     Virtualization.framework, no setuid concerns).
   - **Arch/CachyOS**: `shadow` package usually provides the setuid helpers already.
     Verify `/etc/subuid` + `/etc/subgid`.
3. Verify with `podman info` or `podman run --rm hello-world`.

## Gotchas

- Nix cannot install setuid binaries — `newuidmap`/`newgidmap` must come from the host
  distro's package manager on Linux.
- `podman-compose` is a separate nixpkgs package, not bundled with `podman`.
- On macOS, `podman machine` must be running before any container operations work.
- `loginctl enable-linger $USER` needed for containers to survive logout (we already do
  this for systemd services).

## Scope

Add a new step in the detection/services phase of `ghost init`:

- If no container runtime found, ask: "Install podman via nix? (recommended)"
- If yes, run the install + platform-specific setup
- Re-detect container runtime after install so downstream service prompts can offer
  container options
