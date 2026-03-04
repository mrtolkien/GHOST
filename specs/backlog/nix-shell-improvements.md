# Nix Shell Improvements

## Current Limitations

### 1. Per-command `nix develop` overhead (~0.5s)

Every `run_shell_command` is wrapped in
`nix develop $WORKSPACE/shell/ --command sh -c "..."`. This ensures flake changes take
effect immediately (full autonomy) but adds ~0.5s per command for cached flake
evaluation. Acceptable for a background agent but not ideal.

### 2. Ghost binary not managed by Nix

The ghost binary is baked into the Docker image. Updating ghost requires the OPERATOR to
pull a new image and restart the container. The GHOST cannot update itself.

## Possible Solutions

### For latency: `nix print-dev-env` caching

`nix print-dev-env` outputs shell code that sets up the environment. Cache it:

1. At daemon boot, run `nix print-dev-env $WORKSPACE/shell/` and store the env vars
2. Apply cached env vars to each `Command::new("sh")` via `.envs()`
3. Watch `shell/flake.nix` (file watcher already exists) — on change, re-run
   `nix print-dev-env` and update the cache (~1-2s one-time cost)

Result: zero per-command overhead, flake changes picked up within seconds.

### For latency: daemon-level `nix develop` with re-exec

1. Start daemon inside `nix develop` (zero per-command overhead)
2. File watcher detects flake.nix changes
3. Daemon gracefully re-execs itself inside a new `nix develop` session
4. Ongoing sessions are preserved (DB-backed), only the process restarts

More complex but truly zero overhead.

### For self-update: ghost as a Nix flake package

1. Publish ghost as a Nix flake (fetches pre-built binary from GitHub releases)
2. Workspace flake includes ghost as an input
3. `nix flake update` pulls the latest ghost version
4. Daemon detects the update and re-execs with the new binary

Requires: Nix package definition, CI publishing release binaries, flake overlay.

### For self-update: in-place binary replacement

1. Ghost checks for updates (GitHub releases API)
2. Downloads new binary to a temp path
3. Replaces itself (`/usr/local/bin/ghost`) and re-execs

Simpler than the Nix approach but less deterministic and doesn't work well with Nix
store immutability.
