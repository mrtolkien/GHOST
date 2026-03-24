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
