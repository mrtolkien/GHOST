use crate::error::GhostError;

const FLAKE_REF: &str = "github:mrtolkien/GHOST";

/// Update ghost binary via `nix profile`, then reboot the daemon.
///
/// Default: upgrade the existing profile entry (pulls latest from the
/// flake's default branch or locked ref).
/// --from-source: remove + re-install from main.
/// --version: remove + re-install from a specific tag.
#[tracing::instrument(skip_all)]
pub async fn execute(
    from_source: bool,
    version: Option<String>,
) -> Result<(), GhostError> {
    let old_version = format!(
        "{} ({})",
        env!("CARGO_PKG_VERSION"),
        env!("GIT_COMMIT_HASH")
    );
    println!("current: ghost {old_version}");

    let flake_ref = if from_source {
        format!("{FLAKE_REF}/main")
    } else if let Some(ref tag) = version {
        let tag = if tag.starts_with('v') {
            tag.clone()
        } else {
            format!("v{tag}")
        };
        format!("{FLAKE_REF}/{tag}")
    } else {
        FLAKE_REF.to_string()
    };

    // Remove existing ghost entry (if any) then install the new one.
    // `nix profile remove` matches by flake reference regex, not pname.
    // We match any entry containing "GHOST" (case-insensitive not
    // available, but our flake ref is always uppercase GHOST).
    println!("removing old ghost from nix profile...");
    let _ = std::process::Command::new("nix")
        .args(["profile", "remove", "--regex", ".*GHOST.*"])
        .status();

    println!("installing ghost from {flake_ref}...");
    let status = std::process::Command::new("nix")
        .args(["profile", "add", &flake_ref])
        .status()
        .map_err(|e| {
            std::io::Error::new(e.kind(), format!("failed to run nix: {e}"))
        })?;

    if !status.success() {
        return Err(std::io::Error::other(
            "nix profile add failed — check output above",
        )
        .into());
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
    println!("rebooting daemon...");

    crate::cli::reboot::execute()?;
    Ok(())
}
