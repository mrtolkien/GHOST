use std::process::Command;

use super::OnboardingError;
use super::detect::{ContainerRuntime, DetectedEnvironment};
use super::services::nix_add;

/// Offer to install podman via nix when no container runtime is detected.
///
/// On success, updates `env.container_runtime` to `Some(Podman)`. Every step
/// is lenient: failures warn and continue without a runtime rather than
/// aborting the wizard.
pub fn prompt_container_setup(env: &mut DetectedEnvironment) -> Result<(), OnboardingError> {
    if env.container_runtime.is_some() {
        return Ok(());
    }

    let should_install = cliclack::confirm(
        "No container runtime found. Install podman via nix? \
         (recommended for SearXNG, Crawl4AI, Docling)",
    )
    .initial_value(true)
    .interact()?;

    if !should_install {
        return Ok(());
    }

    // ── Install podman + podman-compose via nix ──

    if let Err(e) = nix_add("podman", "Adding podman via nix...") {
        let _ = cliclack::log::warning(format!("{e}"));
        return Ok(());
    }

    if let Err(e) = nix_add("podman-compose", "Adding podman-compose via nix...") {
        let _ = cliclack::log::warning(format!("{e}"));
        // podman itself is installed, continue with setup
    }

    // ── Platform-specific prerequisites ──

    if env.platform.is_linux() {
        if let Err(e) = setup_linux_prerequisites() {
            let _ = cliclack::log::warning(format!("{e}"));
            let _ = cliclack::log::info("Podman may not work correctly for rootless containers");
        }
    } else if env.platform.is_macos() {
        if let Err(e) = setup_macos_podman_machine() {
            let _ = cliclack::log::warning(format!("{e}"));
            let _ = cliclack::log::info(
                "Run 'podman machine init && podman machine start' manually later",
            );
        }
    }

    // ── Generate container config files ──

    if let Err(e) = ensure_container_configs() {
        let _ = cliclack::log::warning(format!("Could not generate container configs: {e}"));
    }

    // ── Verify ──

    match verify_podman() {
        Ok(()) => {
            env.container_runtime = Some(ContainerRuntime::Podman);
        }
        Err(e) => {
            let _ = cliclack::log::warning(format!("{e}"));
            let _ = cliclack::log::info(
                "Container runtime not available — container services will be skipped",
            );
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Linux prerequisites
// ---------------------------------------------------------------------------

/// Check and guide the user through Linux-specific prerequisites for rootless
/// podman: `newuidmap` with setuid/capabilities, and subuid/subgid entries.
fn setup_linux_prerequisites() -> Result<(), OnboardingError> {
    check_newuidmap()?;
    check_subuid_subgid()?;
    Ok(())
}

/// Verify `newuidmap` exists and has the right privileges (setuid or caps).
fn check_newuidmap() -> Result<(), OnboardingError> {
    let path = find_in_path("newuidmap");

    if path.is_none() {
        let hint = distro_install_hint();
        let _ = cliclack::log::warning(format!(
            "newuidmap not found — needed for rootless containers\n  Run: {hint}"
        ));

        let ready = cliclack::confirm("Continue after installing?")
            .initial_value(true)
            .interact()?;
        if !ready {
            return Ok(());
        }

        // Re-check after user claims they installed it.
        if find_in_path("newuidmap").is_none() {
            let _ = cliclack::log::warning("newuidmap still not found");
        }
        return Ok(());
    }

    // Exists — check privileges.
    let path = path.unwrap();
    if !has_setuid_or_caps(&path) {
        let _ = cliclack::log::warning(format!(
            "newuidmap at {} lacks setuid bit or capabilities — rootless may fail",
            path.display()
        ));
    }

    Ok(())
}

/// Ensure the current user has entries in `/etc/subuid` and `/etc/subgid`.
fn check_subuid_subgid() -> Result<(), OnboardingError> {
    let username = std::env::var("USER").unwrap_or_default();
    if username.is_empty() {
        return Ok(());
    }

    let subuid_ok = file_has_user_entry("/etc/subuid", &username);
    let subgid_ok = file_has_user_entry("/etc/subgid", &username);

    if subuid_ok && subgid_ok {
        return Ok(());
    }

    let _ = cliclack::log::warning(format!(
        "subuid/subgid entries missing for {username}\n  \
         Run: sudo usermod --add-subuids 100000-165535 \
         --add-subgids 100000-165535 {username}"
    ));

    let ready = cliclack::confirm("Continue after running the command?")
        .initial_value(true)
        .interact()?;
    if !ready {
        return Ok(());
    }

    if !file_has_user_entry("/etc/subuid", &username)
        || !file_has_user_entry("/etc/subgid", &username)
    {
        let _ = cliclack::log::warning("subuid/subgid entries still missing");
    }

    Ok(())
}

/// Check if a file contains a line starting with `username:`.
fn file_has_user_entry(path: &str, username: &str) -> bool {
    let prefix = format!("{username}:");
    std::fs::read_to_string(path)
        .map(|content| content.lines().any(|line| line.starts_with(&prefix)))
        .unwrap_or(false)
}

/// Detect the distro family and return the appropriate install command for
/// the `uidmap` / `shadow` package.
fn distro_install_hint() -> &'static str {
    let content = std::fs::read_to_string("/etc/os-release").unwrap_or_default();
    let lower = content.to_lowercase();

    if lower.contains("id=ubuntu")
        || lower.contains("id=debian")
        || lower.contains("id_like=debian")
        || lower.contains("id_like=\"debian")
    {
        return "sudo apt install uidmap";
    }
    if lower.contains("id=fedora") || lower.contains("id_like=fedora") {
        return "sudo dnf install shadow-utils";
    }
    if lower.contains("id=arch") || lower.contains("id_like=arch") || lower.contains("id=cachyos") {
        return "sudo pacman -S shadow";
    }

    "install the 'uidmap' or 'shadow-utils' package for your distro"
}

/// Search PATH for a binary and return its full path.
fn find_in_path(binary: &str) -> Option<std::path::PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(binary))
            .find(|candidate| candidate.is_file())
    })
}

/// Check whether a binary has the setuid bit or file capabilities.
#[cfg(unix)]
fn has_setuid_or_caps(path: &std::path::Path) -> bool {
    use std::os::unix::fs::MetadataExt;

    // Check setuid bit (mode & 0o4000).
    if let Ok(meta) = std::fs::metadata(path) {
        if meta.mode() & 0o4000 != 0 {
            return true;
        }
    }

    // Check file capabilities (Fedora-style: cap_setuid+ep).
    if let Ok(output) = Command::new("getcap").arg(path).output() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        if stdout.contains("cap_setuid") {
            return true;
        }
    }

    false
}

#[cfg(not(unix))]
fn has_setuid_or_caps(_path: &std::path::Path) -> bool {
    false
}

// ---------------------------------------------------------------------------
// macOS: podman machine
// ---------------------------------------------------------------------------

/// Initialize and start a podman machine on macOS.
fn setup_macos_podman_machine() -> Result<(), OnboardingError> {
    let spinner = cliclack::spinner();
    spinner.start("Initializing podman machine (4 CPUs, 6GB RAM, 20GB disk)…");

    let output = Command::new("podman")
        .args([
            "machine",
            "init",
            "--cpus",
            "4",
            "--memory",
            "6144",
            "--disk-size",
            "20",
        ])
        .output()
        .map_err(|e| OnboardingError::HealthCheck(format!("podman machine init: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        spinner.stop("podman machine init failed");
        return Err(OnboardingError::HealthCheck(format!(
            "podman machine init failed: {stderr}"
        )));
    }
    spinner.stop("podman machine initialized");

    let spinner = cliclack::spinner();
    spinner.start("Starting podman machine…");

    let output = Command::new("podman")
        .args(["machine", "start"])
        .output()
        .map_err(|e| OnboardingError::HealthCheck(format!("podman machine start: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        spinner.stop("podman machine start failed");
        return Err(OnboardingError::HealthCheck(format!(
            "podman machine start failed: {stderr}"
        )));
    }
    spinner.stop("podman machine started");

    Ok(())
}

// ---------------------------------------------------------------------------
// Container config files
// ---------------------------------------------------------------------------

/// Generate `~/.config/containers/policy.json` and `registries.conf` if they
/// don't already exist. Some non-NixOS systems lack these and podman refuses
/// to pull images without them.
fn ensure_container_configs() -> Result<(), OnboardingError> {
    let config_dir = dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .join("containers");
    std::fs::create_dir_all(&config_dir)?;

    let policy_path = config_dir.join("policy.json");
    if !policy_path.exists() {
        std::fs::write(
            &policy_path,
            r#"{ "default": [{ "type": "insecureAcceptAnything" }] }
"#,
        )?;
        let _ = cliclack::log::success(format!("Created {}", policy_path.display()));
    }

    let registries_path = config_dir.join("registries.conf");
    if !registries_path.exists() {
        std::fs::write(
            &registries_path,
            "unqualified-search-registries = [\"docker.io\"]\n",
        )?;
        let _ = cliclack::log::success(format!("Created {}", registries_path.display()));
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Verification
// ---------------------------------------------------------------------------

/// Run `podman info` to verify the runtime is working.
fn verify_podman() -> Result<(), OnboardingError> {
    let spinner = cliclack::spinner();
    spinner.start("Verifying podman…");

    let output = Command::new("podman")
        .arg("info")
        .output()
        .map_err(|e| OnboardingError::HealthCheck(format!("podman info: {e}")))?;

    if output.status.success() {
        spinner.stop("podman verified — container runtime ready");
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        spinner.stop("podman verification failed");
        Err(OnboardingError::HealthCheck(format!(
            "podman info failed: {stderr}"
        )))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_has_user_entry_found() {
        let tmp = std::env::temp_dir().join("test_subuid");
        std::fs::write(&tmp, "alice:100000:65536\nbob:200000:65536\n").unwrap();
        assert!(file_has_user_entry(tmp.to_str().unwrap(), "alice"));
        assert!(file_has_user_entry(tmp.to_str().unwrap(), "bob"));
        assert!(!file_has_user_entry(tmp.to_str().unwrap(), "charlie"));
        std::fs::remove_file(&tmp).ok();
    }

    #[test]
    fn file_has_user_entry_missing_file() {
        assert!(!file_has_user_entry("/nonexistent/subuid", "alice"));
    }

    #[test]
    fn distro_hint_fallback() {
        // On any platform, calling the function should not panic.
        let hint = distro_install_hint();
        assert!(!hint.is_empty());
    }
}
