use crate::error::GhostError;

const FLAKE_REF: &str = "github:mrtolkien/GHOST";

/// Update ghost binary via `nix profile`, then reboot the daemon.
///
/// Default: `nix profile upgrade` to atomically update the existing entry.
/// --from-source / --version: remove + add with a specific flake ref
/// (upgrade can't switch branches/tags).
#[tracing::instrument(skip_all)]
pub async fn execute(from_source: bool, version: Option<String>) -> Result<(), GhostError> {
    let old_version = format!(
        "{} ({})",
        env!("CARGO_PKG_VERSION"),
        env!("GIT_COMMIT_HASH")
    );
    println!("current: ghost {old_version}");

    if from_source || version.is_some() {
        // Switching ref — need remove + add
        let flake_ref = if from_source {
            format!("{FLAKE_REF}/main")
        } else {
            let ref_name = version.as_deref().unwrap();
            // Prepend 'v' only for version numbers (e.g. "0.3.0" → "v0.3.0").
            // Branch names like "latest" or "main" are used as-is.
            let ref_name = if ref_name.starts_with(|c: char| c.is_ascii_digit()) {
                format!("v{ref_name}")
            } else {
                ref_name.to_string()
            };
            format!("{FLAKE_REF}/{ref_name}")
        };

        // Pre-build so the add after remove is instant
        println!("building ghost from {flake_ref}...");
        let status = std::process::Command::new("nix")
            .args(["build", "--refresh", "--no-link", &flake_ref])
            .status()
            .map_err(|e| std::io::Error::new(e.kind(), format!("failed to run nix: {e}")))?;

        if !status.success() {
            return Err(std::io::Error::other("nix build failed — old version preserved").into());
        }

        println!("swapping ghost in nix profile...");
        let _ = std::process::Command::new("nix")
            .args(["profile", "remove", "--regex", ".*GHOST.*"])
            .status();

        let status = std::process::Command::new("nix")
            .args(["profile", "add", &flake_ref])
            .status()
            .map_err(|e| std::io::Error::new(e.kind(), format!("failed to run nix: {e}")))?;

        if !status.success() {
            return Err(std::io::Error::other("nix profile add failed").into());
        }
    } else {
        // Default: atomic in-place upgrade
        println!("upgrading ghost...");
        let status = std::process::Command::new("nix")
            .args(["profile", "upgrade", "--refresh", "--regex", ".*GHOST.*"])
            .status()
            .map_err(|e| std::io::Error::new(e.kind(), format!("failed to run nix: {e}")))?;

        if !status.success() {
            return Err(std::io::Error::other("nix profile upgrade failed").into());
        }
    }

    // Read new version from the freshly installed binary
    let new_version = std::process::Command::new("ghost")
        .arg("version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".into());

    println!("updated: {new_version}");

    // Delegate post-update steps to the NEW binary. The old binary
    // running right now may not understand config changes introduced by
    // the new version (e.g. new provider variants), so we exec the new
    // binary for service file regeneration and daemon reboot.
    println!("rebooting daemon...");
    let status = std::process::Command::new("ghost")
        .arg("reboot")
        .status()
        .map_err(|e| std::io::Error::new(e.kind(), format!("failed to reboot daemon: {e}")))?;
    if !status.success() {
        eprintln!("warning: daemon reboot returned non-zero");
    }

    Ok(())
}
