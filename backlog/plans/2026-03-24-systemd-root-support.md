# Systemd Root/System-Level Support

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or superpowers:executing-plans
> to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When running as root (e.g., in an LXC container), use system-level systemd
(`systemctl` without `--user`) instead of user-level, so the daemon and services work
without a D-Bus user session bus.

**Architecture:** A new `src/systemd.rs` module centralizes all systemd interaction
behind helpers that detect root via `getuid() == 0` and conditionally toggle between
`--user` (user units in `~/.config/systemd/user/`) and system-level (units in
`/etc/systemd/system/`). Every callsite that currently hardcodes `--user` is replaced
with calls to these helpers.

**Tech Stack:** Rust std (`Command`), `libc` crate (for `getuid`), systemd

---

## File Structure

| File                              | Action     | Responsibility                                                                                             |
| --------------------------------- | ---------- | ---------------------------------------------------------------------------------------------------------- |
| `Cargo.toml`                      | **Modify** | Add `libc` dependency                                                                                      |
| `src/systemd.rs`                  | **Create** | All systemd helpers: `is_root()`, `systemctl_run()`, `unit_dir()`, `is_unit_active()`                      |
| `src/lib.rs`                      | **Modify** | Add `pub mod systemd;`                                                                                     |
| `src/onboarding/health.rs`        | **Modify** | Replace `run_systemctl()` and `is_daemon_active()` with `systemd::` calls                                  |
| `src/onboarding/service_files.rs` | **Modify** | Use `systemd::unit_dir()` for install path; conditionally skip linger; fix unit templates for system-level |
| `src/onboarding/services.rs`      | **Modify** | Use `systemd::` helpers in `native_service_entry()` for generated shell commands                           |
| `src/cli/start_stop.rs`           | **Modify** | Replace hardcoded `--user` with `systemd::` calls                                                          |
| `src/cli/status.rs`               | **Modify** | Replace `is_systemd_active()` with `systemd::is_unit_active()`                                             |
| `src/cli/reset.rs`                | **Modify** | Replace `stop_systemd()` and `remove_service_files()` systemd paths with `systemd::` calls                 |
| `src/cli/reload.rs`               | **Modify** | Replace hardcoded `--user` with `systemd::` call                                                           |
| `src/cli/reboot.rs`               | **Modify** | Replace hardcoded `--user` with `systemd::` calls                                                          |

---

### Task 1: Create `src/systemd.rs` with core helpers

**Files:**

- Modify: `Cargo.toml`
- Create: `src/systemd.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Add `libc` to `Cargo.toml` dependencies**

Add under `[dependencies]` (alphabetical):

```toml
libc = "0.2"
```

- [ ] **Step 2: Create `src/systemd.rs` with all helpers**

```rust
//! Systemd helpers — centralises user-vs-system unit logic.
//!
//! When running as root (UID 0, typical in LXC containers), systemd user
//! services don't work because there is no D-Bus user session bus. In that
//! case we use system-level units instead.

use std::path::PathBuf;
use std::process::Command;

/// Returns `true` when the process is running as root (UID 0).
///
/// Root cannot use `systemctl --user` because the user session bus is
/// typically unavailable (no `$DBUS_SESSION_BUS_ADDRESS`).
pub fn is_root() -> bool {
    // SAFETY: getuid() is always safe to call and has no failure mode.
    unsafe { libc::getuid() == 0 }
}

/// Run `systemctl` with the correct scope (system or `--user`).
///
/// When root, runs system-level; otherwise runs `--user`. Returns the
/// `std::process::Output` or an IO error.
pub fn systemctl_run(args: &[&str]) -> std::io::Result<std::process::Output> {
    let mut cmd = Command::new("systemctl");
    if !is_root() {
        cmd.arg("--user");
    }
    cmd.args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
}

/// Run `systemctl` with the correct scope, returning just the exit status.
///
/// Suppresses stdout/stderr — use for fire-and-forget operations.
pub fn systemctl_status(args: &[&str]) -> std::io::Result<std::process::ExitStatus> {
    let mut cmd = Command::new("systemctl");
    if !is_root() {
        cmd.arg("--user");
    }
    cmd.args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
}

/// Check whether a systemd unit is active (running).
pub fn is_unit_active(unit: &str) -> bool {
    systemctl_status(&["is-active", unit])
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Enable and start a systemd unit. Best-effort — ignores failures.
pub fn enable_now(unit: &str) {
    let _ = systemctl_status(&["enable", "--now", unit]);
}

/// Disable and stop a systemd unit. Best-effort — ignores failures.
pub fn disable_now(unit: &str) {
    let _ = systemctl_status(&["disable", "--now", unit]);
}

/// Reload the systemd daemon so it picks up new/removed unit files.
pub fn daemon_reload() {
    let _ = systemctl_status(&["daemon-reload"]);
}

/// Start a systemd unit. Returns an error message on failure.
pub fn start(unit: &str) -> Result<(), String> {
    let status = systemctl_status(&["start", unit])
        .map_err(|e| format!("failed to run systemctl: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("systemctl start {unit} failed"))
    }
}

/// Directory where unit files should be installed.
///
/// - Root: `/etc/systemd/system/`
/// - Non-root: `~/.config/systemd/user/`
pub fn unit_dir() -> std::io::Result<PathBuf> {
    if is_root() {
        Ok(PathBuf::from("/etc/systemd/system"))
    } else {
        dirs::config_dir()
            .map(|d| d.join("systemd/user"))
            .ok_or_else(|| std::io::Error::other("cannot determine config directory"))
    }
}

/// Build a `systemctl` shell command string with the correct scope prefix.
///
/// Used when generating persistent shell commands (e.g., services.toml entries)
/// that must include `--user` or not depending on root status at generation time.
///
/// Example: `systemctl_shell_cmd("start ghost-daemon")` returns
/// `"systemctl start ghost-daemon"` for root, or
/// `"systemctl --user start ghost-daemon"` for non-root.
pub fn systemctl_shell_cmd(args: &str) -> String {
    if is_root() {
        format!("systemctl {args}")
    } else {
        format!("systemctl --user {args}")
    }
}
```

- [ ] **Step 3: Add module declaration to `src/lib.rs`**

Add `pub mod systemd;` in alphabetical order (after `services`).

- [ ] **Step 4: Verify it compiles**

Run: `cargo check 2>&1 | head -20` Expected: no errors (module is defined but not yet
used — warnings are fine).

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock src/systemd.rs src/lib.rs
git commit -m "feat: add systemd helpers for root/system-level support"
```

---

### Task 2: Update unit file templates and installation

**Files:**

- Modify: `src/onboarding/service_files.rs`

The unit file generators need changes for system-level:

1. Replace `%h` (user home specifier, not supported in system units) with the actual
   home path.
2. Use `WantedBy=multi-user.target` instead of `default.target`.
3. Skip `ensure_linger_enabled()` when root (system services don't need linger).
4. Use `systemd::unit_dir()` for the install path.

- [ ] **Step 1: Update `generate_daemon_unit_systemd` to accept `system_level` flag**

Change the signature to
`pub fn generate_daemon_unit_systemd(exe: &str, workspace: &str, system_level: bool) -> String`.

When `system_level` is true:

- Replace `%h` with the value from `dirs::home_dir()` (or `/root` fallback).
- Use `WantedBy=multi-user.target`.

When false, keep existing behaviour (`%h`, `WantedBy=default.target`).

```rust
pub fn generate_daemon_unit_systemd(exe: &str, workspace: &str, system_level: bool) -> String {
    let (home, target) = if system_level {
        let home = dirs::home_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("/root"))
            .display()
            .to_string();
        (home, "multi-user.target")
    } else {
        ("%h".to_string(), "default.target")
    };

    format!(
        r#"[Unit]
Description=GHOST AI Agent Daemon
After=network-online.target

[Service]
ExecStart={exe} daemon
WorkingDirectory={workspace}
Environment=PATH=/nix/var/nix/profiles/default/bin:{home}/.nix-profile/bin:/usr/local/bin:/usr/bin:/bin
Restart=always
RestartSec=2
TimeoutStopSec=120

[Install]
WantedBy={target}
"#
    )
}
```

- [ ] **Step 2: Update `generate_llama_server_unit_systemd` the same way**

Same pattern: add `system_level: bool` parameter, swap `%h` and `WantedBy`.

```rust
pub fn generate_llama_server_unit_systemd(
    exe: &str,
    hf_repo: &str,
    alias: &str,
    system_level: bool,
) -> String {
    let (home, target) = if system_level {
        let home = dirs::home_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("/root"))
            .display()
            .to_string();
        (home, "multi-user.target")
    } else {
        ("%h".to_string(), "default.target")
    };

    format!(
        r#"[Unit]
Description=llama-server embedding service
After=network-online.target

[Service]
ExecStart={exe} --embedding --hf-repo {hf_repo} --alias {alias} --port 11434
Restart=on-failure
RestartSec=5
TimeoutStopSec=30
Environment=PATH=/nix/var/nix/profiles/default/bin:{home}/.nix-profile/bin:/usr/local/bin:/usr/bin:/bin

[Install]
WantedBy={target}
"#
    )
}
```

- [ ] **Step 3: Update `install_all_service_files` Linux branch**

Replace the unit dir lookup:

```rust
// Before:
let unit_dir = dirs::config_dir()
    .ok_or_else(|| std::io::Error::other("cannot determine config directory"))?
    .join("systemd/user");

// After:
let unit_dir = crate::systemd::unit_dir()?;
```

Update both generator callsites to pass the `system_level` flag:

```rust
// Before (line 274):
std::fs::write(&path, generate_daemon_unit_systemd(exe, workspace))?;
// After:
std::fs::write(&path, generate_daemon_unit_systemd(exe, workspace, crate::systemd::is_root()))?;

// Before (line 282):
std::fs::write(&path, generate_llama_server_unit_systemd(ls.exe, ls.hf_repo, ls.alias))?;
// After:
std::fs::write(&path, generate_llama_server_unit_systemd(ls.exe, ls.hf_repo, ls.alias, crate::systemd::is_root()))?;
```

Guard `ensure_linger_enabled()`:

```rust
// Before:
ensure_linger_enabled();
// After:
if !crate::systemd::is_root() {
    ensure_linger_enabled();
}
```

- [ ] **Step 4: Update the existing tests**

The existing tests call `generate_daemon_unit_systemd` and
`generate_llama_server_unit_systemd` — update them to pass `false` for `system_level` so
they keep testing the user-unit path. Add new tests for `system_level: true` that assert
`multi-user.target` is present and `%h` is absent.

```rust
#[test]
fn daemon_unit_system_level() {
    let unit = generate_daemon_unit_systemd("/usr/bin/ghost", "/home/user/GHOST", true);
    assert!(unit.contains("WantedBy=multi-user.target"));
    assert!(!unit.contains("%h"));
}

#[test]
fn llama_server_unit_system_level() {
    let unit = generate_llama_server_unit_systemd(
        "/usr/bin/llama-server",
        "Qwen/Qwen3-Embedding-8B-GGUF:Q8_0",
        "qwen3-embedding:8b",
        true,
    );
    assert!(unit.contains("WantedBy=multi-user.target"));
    assert!(!unit.contains("%h"));
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test --lib onboarding::service_files 2>&1 | tail -20` Expected: all tests
pass.

- [ ] **Step 6: Commit**

```bash
git add src/onboarding/service_files.rs
git commit -m "feat: support system-level systemd units when running as root"
```

---

### Task 3: Update onboarding health checks

**Files:**

- Modify: `src/onboarding/health.rs`

- [ ] **Step 1: Replace `run_systemctl` with `systemd::enable_now`**

Delete the `run_systemctl` function (lines 215–219). Replace its two callsites:

```rust
// Before:
run_systemctl("ghost-daemon");
// ...
run_systemctl("llama-server");

// After:
crate::systemd::enable_now("ghost-daemon");
// ...
crate::systemd::enable_now("llama-server");
```

- [ ] **Step 2: Replace `is_daemon_active` with `systemd::is_unit_active`**

The `is_daemon_active` function (lines 291–316) has both macOS and Linux branches.
Replace only the Linux (`else`) branch:

```rust
fn is_daemon_active() -> bool {
    if cfg!(target_os = "macos") {
        // ... existing launchctl code unchanged ...
    } else {
        crate::systemd::is_unit_active("ghost-daemon")
    }
}
```

- [ ] **Step 3: Also update `start_systemd_services` unit dir path**

The `unit_dir` in `start_systemd_services` (line 203) is used to check if
`llama-server.service` exists. Update it to use `systemd::unit_dir()`. The fallback here
is only hit when `dirs::config_dir()` returns `None` for non-root — keep a sensible
non-root fallback:

```rust
// Before:
let unit_dir = dirs::config_dir()
    .unwrap_or_else(|| std::path::PathBuf::from("/etc/xdg"))
    .join("systemd/user");

// After:
let unit_dir = crate::systemd::unit_dir()
    .unwrap_or_else(|_| std::path::PathBuf::from("/etc/xdg/systemd/user"));
```

(The fallback is only reached for non-root users when `$XDG_CONFIG_HOME` is undefined.
`/etc/xdg/systemd/user` is no better than the old `/etc/xdg` but this is a "unit file
won't be found, skip it" path — not a write path.)

- [ ] **Step 4: Verify it compiles**

Run: `cargo check 2>&1 | head -20` Expected: no errors.

- [ ] **Step 5: Commit**

```bash
git add src/onboarding/health.rs
git commit -m "refactor: use systemd helpers in onboarding health checks"
```

---

### Task 4: Update `ghost start` / `ghost stop`

**Files:**

- Modify: `src/cli/start_stop.rs`

- [ ] **Step 1: Replace `start_daemon` Linux impl**

Replace lines 96–109:

```rust
#[cfg(not(target_os = "macos"))]
fn start_daemon() -> Result<(), GhostError> {
    crate::systemd::start("ghost-daemon")
        .map_err(|msg| GhostError::Other(msg.into()))
}
```

- [ ] **Step 2: Replace `stop_daemon` Linux impl**

Replace lines 135–149:

```rust
#[cfg(not(target_os = "macos"))]
fn stop_daemon() {
    let ok = crate::systemd::systemctl_status(&["disable", "--now", "ghost-daemon"])
        .is_ok_and(|s| s.success());

    if !ok {
        eprintln!(
            "  Warning: systemctl disable --now may have failed (daemon may not have been running)"
        );
    }
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check 2>&1 | head -20` Expected: no errors.

- [ ] **Step 4: Commit**

```bash
git add src/cli/start_stop.rs
git commit -m "refactor: use systemd helpers in start/stop commands"
```

---

### Task 5: Update `ghost status`

**Files:**

- Modify: `src/cli/status.rs`

- [ ] **Step 1: Replace `is_systemd_active` with `systemd::is_unit_active`**

Delete the `is_systemd_active` function (lines 75–83). Update `is_service_active`:

```rust
fn is_service_active() -> bool {
    if cfg!(target_os = "macos") {
        is_launchd_active()
    } else {
        crate::systemd::is_unit_active("ghost-daemon")
    }
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check 2>&1 | head -20` Expected: no errors.

- [ ] **Step 3: Commit**

```bash
git add src/cli/status.rs
git commit -m "refactor: use systemd helpers in status command"
```

---

### Task 6: Update `ghost reset`

**Files:**

- Modify: `src/cli/reset.rs`

- [ ] **Step 1: Replace `stop_systemd` with inline calls using `systemd::` helpers**

Delete the `stop_systemd` function (lines 154–166). Replace its callsites with inline
calls that preserve the "Stopped" output:

```rust
// In stop_services() — replace stop_systemd("ghost-daemon"):
if crate::systemd::systemctl_status(&["disable", "--now", "ghost-daemon"])
    .is_ok_and(|s| s.success())
{
    println!("  Stopped ghost-daemon");
}

// In stop_services_legacy() — replace stop_systemd("llama-server") and stop_systemd("docling-serve"):
if crate::systemd::systemctl_status(&["disable", "--now", "llama-server"])
    .is_ok_and(|s| s.success())
{
    println!("  Stopped llama-server");
}
if crate::systemd::systemctl_status(&["disable", "--now", "docling-serve"])
    .is_ok_and(|s| s.success())
{
    println!("  Stopped docling-serve");
}
```

- [ ] **Step 2: Update `remove_service_files` Linux path and daemon-reload**

Replace:

```rust
let unit_dir = dirs::config_dir()
    .unwrap_or_else(|| PathBuf::from("/etc/xdg"))
    .join("systemd/user");
```

With:

```rust
let unit_dir = crate::systemd::unit_dir()
    .unwrap_or_else(|_| PathBuf::from("/etc/xdg/systemd/user"));
```

Replace:

```rust
let _ = Command::new("systemctl")
    .args(["--user", "daemon-reload"])
    .status();
```

With:

```rust
crate::systemd::daemon_reload();
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check 2>&1 | head -20` Expected: no errors.

- [ ] **Step 4: Commit**

```bash
git add src/cli/reset.rs
git commit -m "refactor: use systemd helpers in reset command"
```

---

### Task 7: Update `ghost reload`

**Files:**

- Modify: `src/cli/reload.rs`

- [ ] **Step 1: Replace the Linux branch systemctl call**

In `execute()`, replace lines 34–37:

```rust
// Before:
let status = std::process::Command::new("systemctl")
    .args(["--user", "kill", "--signal=SIGHUP", "ghost-daemon"])
    .status()
    .map_err(|e| std::io::Error::new(e.kind(), format!("failed to run systemctl: {e}")))?;

// After:
let status = crate::systemd::systemctl_status(&["kill", "--signal=SIGHUP", "ghost-daemon"])
    .map_err(|e| std::io::Error::new(e.kind(), format!("failed to run systemctl: {e}")))?;
```

Note: `systemctl_status` suppresses stdout/stderr and returns `ExitStatus`, which
matches the existing usage (it only checks `.success()` on the next line). However,
`systemctl_status` returns `io::Result<ExitStatus>` while the old code also returns
`io::Result<ExitStatus>`, so this is a drop-in replacement.

- [ ] **Step 2: Verify it compiles**

Run: `cargo check 2>&1 | head -20` Expected: no errors.

- [ ] **Step 3: Commit**

```bash
git add src/cli/reload.rs
git commit -m "refactor: use systemd helpers in reload command"
```

---

### Task 8: Update `ghost reboot`

**Files:**

- Modify: `src/cli/reboot.rs`

- [ ] **Step 1: Replace both Linux branch systemctl calls**

In `execute()`, replace lines 22–30:

```rust
// Before:
// Reload unit files in case the service file was regenerated
let _ = std::process::Command::new("systemctl")
    .args(["--user", "daemon-reload"])
    .status();

let status = std::process::Command::new("systemctl")
    .args(["--user", "restart", "ghost-daemon"])
    .status()
    .map_err(|e| std::io::Error::new(e.kind(), format!("failed to run systemctl: {e}")))?;

// After:
// Reload unit files in case the service file was regenerated
crate::systemd::daemon_reload();

let status = crate::systemd::systemctl_status(&["restart", "ghost-daemon"])
    .map_err(|e| std::io::Error::new(e.kind(), format!("failed to run systemctl: {e}")))?;
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check 2>&1 | head -20` Expected: no errors.

- [ ] **Step 3: Commit**

```bash
git add src/cli/reboot.rs
git commit -m "refactor: use systemd helpers in reboot command"
```

---

### Task 9: Update `native_service_entry` in onboarding services

**Files:**

- Modify: `src/onboarding/services.rs`

The `native_service_entry` function (line 556) generates persistent shell command
strings like `"systemctl --user start {name}"` that get written to `services.toml`.
These must also be root-aware.

- [ ] **Step 1: Replace hardcoded `systemctl --user` strings with
      `systemctl_shell_cmd()`**

In `native_service_entry`, replace the `Platform::Linux` arm:

```rust
// Before (lines 562–567):
Platform::Linux => ServiceEntry {
    start: Some(format!("systemctl --user start {service_name}")),
    stop: Some(format!("systemctl --user disable --now {service_name}")),
    update: Some(format!("nix profile upgrade nixpkgs#{nix_package}")),
    status: Some(format!("systemctl --user is-active {service_name}")),
},

// After:
Platform::Linux => ServiceEntry {
    start: Some(crate::systemd::systemctl_shell_cmd(&format!("start {service_name}"))),
    stop: Some(crate::systemd::systemctl_shell_cmd(&format!("disable --now {service_name}"))),
    update: Some(format!("nix profile upgrade nixpkgs#{nix_package}")),
    status: Some(crate::systemd::systemctl_shell_cmd(&format!("is-active {service_name}"))),
},
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check 2>&1 | head -20` Expected: no errors.

- [ ] **Step 3: Commit**

```bash
git add src/onboarding/services.rs
git commit -m "fix: generate root-aware systemctl commands in services.toml"
```

---

### Task 10: Final verification

- [ ] **Step 1: Grep for any remaining `--user` hardcodes in Rust source**

Run: `rg '"--user"' src/ --type rust` Expected: no matches (all replaced).

If the `services.rs` **test fixture** (line ~273) still has `systemctl --user` in test
TOML data, that's fine — it's testing the TOML parser, not generating commands. Leave
it.

- [ ] **Step 2: Run `just ci`**

Run: `just ci` Expected: format, check, clippy, and all tests pass.

- [ ] **Step 3: Fix any issues**

Address clippy lints, unused imports, dead code warnings.

- [ ] **Step 4: Final commit if needed**

```bash
git commit -m "chore: fix lint issues from systemd refactor"
```
