Current deployment is faulty and messy.

Running GHOST inside Docker with nix is a mess:

- Updates fail
- We have a dirty "bootstrap flake"
- It can't manage the other services it needs

---

I want to do a FULL REVIEW of how we deploy our stack.

The goals are:

- Deploying the stack is as easy as possible
- The GHOST's shell is defined with nix _and_ includes the CLI binary that is the same
  version as the running daemon
- Updating the GHOST's is simple and grabs a tagged binary from Github
- There is way to point the GHOST's update system (nix?) to build the new binary from
  `main`

I am open to ANY changes, including:

- Running GHOST _natively_ on the machine and not in Docker
- Having the daemon binary be handled differently than the CLI binary, as long as we can
  _guarantee_ they are the same version
  - But ideally, there would be a single binary installed on the system and used both to
    start the daemon and inside the GHOST's shell
- Moving to podman rootless (we have a spec talking about that) to still have an inkling
  of security
- ANYTHING YOU CAN THINK OF

As secondary goals, I think we need:

- A ghost version command that prints the Cargo.toml version _and_ the git commit it was
  built against
- A way to properly test our deployment and update strategy: we'll use my proxmox server
  to spin up as many VMs/LXCs as you need

---

The deliverable is a way to update the stack on my homelab, currently deployed with
`./private-scripts/homelab.sh deploy`. You can inspect the running GHOST.

---

## Design: Native Binary via Nix Profile

### Problem

Ghost runs inside Docker with Nix. This is broken:

- The root `flake.nix` downloads a pre-built tarball from GitHub Releases. Nix locks
  the hash in `flake.lock`, so `latest` doesn't actually update — you must `nix flake
  update` and rebuild the Docker image.
- Ghost can't manage sibling containers from inside Docker.
- Nix-in-Docker is slow (builds on every container start) and fragile.
- The shell flake references the root flake to get the ghost binary — two-flake
  indirection where version coupling is fragile.
- No `ghost update` command. Updates require manual Docker image pull or homelab.sh.
- `ghost version` only prints Cargo.toml version, no git commit.

### Solution

Ghost runs **natively on the host**. One binary, installed via Nix profile. The shell
flake provides dev tools only — ghost is on PATH via the profile.

**Install flow:**

```
1. Install Nix (one-time, system-level)
2. nix profile install github:mrtolkien/GHOST      # or /v0.2.0 for a tag
3. ghost init                                        # workspace + service file
4. systemctl --user enable --now ghost-daemon        # Linux
   launchctl bootstrap gui/$(id -u) <plist>          # macOS
```

### Architecture

```
Host
├── ~/.nix-profile/bin/ghost      ← nix profile install
├── ghost-daemon.service          ← written by ghost init
├── ~/GHOST/
│   ├── shell/flake.nix           ← tools only (git, python, uv, ...)
│   └── (workspace files)
└── Podman/Docker (sidecar services, out of scope for this spec)
```

**PATH resolution for agent shell commands** (validated on host):

```
1. buildEnv/bin   → git, python, uv, sqlite, curl (from shell flake)
2. ~/.nix-profile → ghost (from nix profile)
3. system PATH    → everything else
```

The daemon calls `run_nix_shell_setup()` at boot, which runs `nix build
$WORKSPACE/shell` and caches the store path. Every `run_shell_command` prepends
`${store_path}/bin` to PATH. Ghost comes from `~/.nix-profile/bin/` further down PATH.
One binary, guaranteed same version.

### Component 1: Root Flake (`flake.nix`)

Builds ghost from source via `buildRustPackage`. Replaces the tarball-downloading
bootstrap flake.

```nix
{
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      supportedSystems = [ "x86_64-linux" "aarch64-linux" "aarch64-darwin" ];
      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
    in {
      packages = forAllSystems (system:
        let pkgs = nixpkgs.legacyPackages.${system}; in {
          default = pkgs.rustPlatform.buildRustPackage {
            pname = "ghost";
            src = self;
            cargoLock.lockFile = ./Cargo.lock;
          };
        }
      );
    };
}
```

- `nix profile install github:mrtolkien/GHOST` builds from main.
- `nix profile install github:mrtolkien/GHOST/v0.2.0` builds from a tag.
- First install compiles (~3-5 min). Cachix binary cache can be added later for instant
  downloads. Nixpkgs submission when stable (free Hydra builds).
- `ghost-bin` tarball input is deleted.
- `aarch64-darwin` added for macOS Apple Silicon.
- `buildRustPackage` will need `nativeBuildInputs` for native dependencies (pkg-config,
  cmake) and `buildInputs` for libraries (sqlite is bundled via `libsqlite3-sys`, but
  rustls/ring may need perl/cmake). Exact inputs to be determined during implementation.

### Component 2: Shell Flake (`assets/shell/flake.nix`)

Tools only. Ghost is NOT an input — it comes from the nix profile via PATH.

```nix
{
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { nixpkgs, ... }:
    let
      mkEnv = system:
        let pkgs = nixpkgs.legacyPackages.${system};
        in pkgs.buildEnv {
          name = "ghost-shell";
          paths = with pkgs; [
            git gh curl wget jq ripgrep fd tree
            coreutils findutils bash gnugrep gnused gawk
            diffutils file less unzip gzip gnutar
            uv python314
            sqlite-interactive
          ];
        };
    in {
      packages.x86_64-linux.default = mkEnv "x86_64-linux";
      packages.aarch64-linux.default = mkEnv "aarch64-linux";
      packages.aarch64-darwin.default = mkEnv "aarch64-darwin";
    };
}
```

### Component 3: `ghost init` (enhanced)

Adds platform-specific service file generation to the existing workspace bootstrap.

**Linux** — writes `~/.config/systemd/user/ghost-daemon.service`:

```ini
[Unit]
Description=GHOST AI Agent Daemon
After=network-online.target

[Service]
ExecStart={current_exe} daemon
Restart=always
RestartSec=2

[Install]
WantedBy=default.target
```

**macOS** — writes `~/Library/LaunchAgents/com.ghost.daemon.plist`:

```xml
<plist version="1.0">
<dict>
  <key>Label</key><string>com.ghost.daemon</string>
  <key>ProgramArguments</key>
  <array>
    <string>{current_exe}</string>
    <string>daemon</string>
  </array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>StandardOutPath</key>
  <string>{data_dir}/logs/ghost-daemon.log</string>
  <key>StandardErrorPath</key>
  <string>{data_dir}/logs/ghost-daemon.err</string>
</dict>
</plist>
```

Uses `std::env::current_exe()` to get the binary path — always correct, no
substitution needed. Templates live as string constants in Rust code, not external
files.

Prints next steps after writing:

```
Service file written. Start the daemon with:
  systemctl --user enable --now ghost-daemon    # Linux
  launchctl bootstrap gui/$(id -u) <path>       # macOS
```

### Component 4: `ghost update`

```
ghost update                       # latest tagged release
ghost update --from-source         # build from main
ghost update --version v0.3.0      # specific tag
```

Implementation:

1. Record current version.
2. Run the appropriate `nix profile` command to swap the binary:
   - Default: `nix profile upgrade` matching the ghost entry
   - `--from-source`: remove + `nix profile add github:mrtolkien/GHOST/main`
   - `--version`: remove + `nix profile add github:mrtolkien/GHOST/v0.3.0`
   Note: exact `nix profile` sub-command syntax (add/install, upgrade pattern matching)
   depends on Nix version and must be verified during implementation. The concept is:
   atomically replace the ghost entry in the profile.
3. Print old → new version.
4. Run `ghost reboot` (SIGTERM → systemd/launchd restarts with new binary).

No `self_update` crate needed. Nix handles fetching, building, and atomic replacement.
Safe to run from within the daemon (via `run_shell_command` background mode): the
running process keeps the old binary in memory, `nix profile` swaps the symlink, then
SIGTERM triggers a restart with the new binary. Unix orphan semantics ensure the update
process completes even after the daemon exits.

### Component 5: `ghost version`

```
$ ghost version
ghost 0.2.0 (abc1234)
```

`build.rs` addition — embed git commit at compile time:

```rust
// Track git state for rebuild triggers
println!("cargo:rerun-if-changed=.git/HEAD");
if let Ok(head) = std::fs::read_to_string(".git/HEAD") {
    if let Some(ref_path) = head.strip_prefix("ref: ") {
        println!("cargo:rerun-if-changed=.git/{}", ref_path.trim());
    }
}

let hash = std::process::Command::new("git")
    .args(["rev-parse", "--short", "HEAD"])
    .output()
    .ok()
    .filter(|o| o.status.success())
    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    .unwrap_or_else(|| "unknown".into());
println!("cargo:rustc-env=GIT_COMMIT_HASH={hash}");
```

The `rerun-if-changed` directives ensure the hash updates on new commits without
triggering rebuilds on unrelated changes. Hash is guaranteed accurate in nix/CI builds
(always clean). In dev builds it tracks `.git/HEAD` and the current branch ref.

`main.rs`:

```rust
Commands::Version => {
    println!("ghost {} ({})", env!("CARGO_PKG_VERSION"), env!("GIT_COMMIT_HASH"));
    Ok(())
}
```

### Component 6: Nix-Shell Skill Update

`assets/skills/nix-shell/skill.md` updated to reflect:

- Self-update is now `ghost update` (not `nix flake update` on the shell dir).
- Ghost is not in the shell flake — it comes from the nix profile.
- Docker/container references removed (ghost runs natively).
- Shell flake update (`nix flake update --flake $WORKSPACE/shell/`) only updates dev
  tools, not the ghost binary.

### Code Changes Summary

| File | Change |
|------|--------|
| `flake.nix` | Rewrite: `buildRustPackage` from source, delete `ghost-bin` input |
| `flake.lock` | Regenerated (only nixpkgs, no ghost-bin) |
| `assets/shell/flake.nix` | Remove ghost input, tools only |
| `build.rs` | Add git commit hash embedding |
| `src/main.rs` | Update `Commands::Version` to print commit hash |
| `src/cli/mod.rs` | Add `update` module |
| `src/cli/update.rs` | New: wraps `nix profile upgrade` + reboot |
| `src/cli/init.rs` | Add service file generation (systemd/launchd) |
| `src/tools/shell.rs:81` | Delete `/usr/local/bin:` from PATH construction (ghost is on PATH via nix profile; no replacement needed) |
| `assets/skills/nix-shell/skill.md` | Rewrite for new architecture |

### What Gets Deleted

- `deploy/common/Dockerfile` — ghost no longer runs in Docker
- `deploy/common/entrypoint.sh` — no more Nix-in-Docker bootstrap
- `docker-compose.yml` (root) — ghost removed from compose; sidecar-only compose
  deferred to onboarding spec
- `docker-compose.local.yml` — no longer needed

### Out of Scope

- Onboarding wizard (see `onboarding_and_services.md`)
- Sidecar service management (see `onboarding_and_services.md`)
- Per-platform service fallbacks (see `deployment_per_platform.md`)
- Cachix binary cache setup (optimization, add later)
- CI workflow changes (may need updates but can be a follow-up)
