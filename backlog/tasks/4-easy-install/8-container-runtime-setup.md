# Auto-install container runtime during onboarding

If `ghost init` detects no container runtime (podman or docker), offer to install podman
rootless via nix and configure it.

## Why

Several services (SearXNG, Crawl4AI, Docling) run as containers. Without a container
runtime, those options are unavailable and the wizard defaults to skipping them. Setting
up podman automatically would make the full service stack accessible out of the box.

## Design

New module: `src/onboarding/container_setup.rs`. Called from the wizard between
detection display and the services phase. Non-blocking — if any step fails, warn and
continue without a container runtime (user can still choose Remote/Skip for services).

### Flow

```
◇  No container runtime found (podman or docker)
│
◆  Install podman via nix? (recommended for SearXNG, Crawl4AI, Docling)
│  Yes
│
●  Adding podman via nix... ✓
●  Adding podman-compose via nix... ✓
│
│  [Linux only, if newuidmap missing:]
│  ⚠ newuidmap not found — needed for rootless containers
│    Run: sudo apt install uidmap
│
│  ◆ Continue after installing?  Yes
│
│  [Linux only, if subuid/subgid missing:]
│  ⚠ subuid/subgid entries missing for your user
│    Run: sudo usermod --add-subuids 100000-165535 --add-subgids 100000-165535 <user>
│
│  ◆ Continue after running the command?  Yes
│
│  [macOS only:]
│  ●  Initializing podman machine (2 CPUs, 4GB RAM, 20GB disk)... ✓
│  ●  Starting podman machine... ✓
│
●  Verifying: podman info... ✓
```

### What nix handles

- `nix profile add nixpkgs#podman` — binary + all helpers (conmon, crun, netavark, etc.)
- `nix profile add nixpkgs#podman-compose` — compose support

### What nix CANNOT handle (requires host/sudo)

**Linux only** — podman rootless needs setuid `newuidmap`/`newgidmap`:

| Distro        | Package               | Pre-installed? | Our action                       |
| ------------- | --------------------- | -------------- | -------------------------------- |
| Arch/CachyOS  | `shadow` (base)       | Yes            | Just verify                      |
| Fedora        | `shadow-utils` (Core) | Yes            | Just verify                      |
| Debian/Ubuntu | `uidmap`              | No             | Guide: `sudo apt install uidmap` |

Also needs `/etc/subuid` + `/etc/subgid` entries — usually auto-created by `useradd`,
but verify and guide if missing.

**macOS** — `podman machine init && podman machine start`. Fully automatable, no sudo.

### Error handling

Every step is lenient:

- Nix add fails → show error, continue without container runtime
- Prerequisites missing and user can't fix → warn, continue without
- Podman machine fails on macOS → show error, continue without
- Verification fails → warn, continue without

After success: update `env.container_runtime = Some(Podman)` so downstream service
prompts offer container options.

### Distro detection

Read `/etc/os-release` for `ID=` and `ID_LIKE=` to determine install hint.

### Container config files

Generate `~/.config/containers/policy.json` and `registries.conf` if not present — some
non-NixOS systems don't have these and podman refuses to pull without them.
