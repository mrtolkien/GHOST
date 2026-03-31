use std::path::Path;

/// Name of the file that stores the nix shell `bin/` path in the workspace.
pub const SHELL_BIN_FILE: &str = ".shell-bin";

/// Read the cached nix shell `bin/` path from `$WORKSPACE/.shell-bin`.
///
/// Returns `None` if the file doesn't exist, is empty, or can't be read.
pub fn read_shell_bin(workspace: &Path) -> Option<String> {
    let path = workspace.join(SHELL_BIN_FILE);
    std::fs::read_to_string(path).ok().and_then(|s| {
        let trimmed = s.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    })
}

/// Build a PATH string with the workspace's nix shell `bin/` prepended.
///
/// If `.shell-bin` doesn't exist, returns the system PATH unchanged.
pub fn nix_path(workspace: &Path) -> String {
    let current_path = std::env::var("PATH").unwrap_or_default();
    match read_shell_bin(workspace) {
        Some(nix_bin) => format!("{nix_bin}:{current_path}"),
        None => current_path,
    }
}

/// Create a `tokio::process::Command` for `program` with the workspace's nix
/// shell PATH prepended.
///
/// Falls back to the system PATH if `.shell-bin` doesn't exist.
pub fn command(workspace: &Path, program: &str) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new(program);
    cmd.env("PATH", nix_path(workspace));
    cmd
}
