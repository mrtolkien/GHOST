use std::path::Path;

use crate::config::{Config, ConfigError};

/// Phase 1: Create workspace directories and user-only identity files.
/// Does NOT install bundled files (skills, agents, etc.).
#[tracing::instrument(skip_all, fields(workspace = %config.workspace.display()))]
pub fn bootstrap_workspace_dirs(config: &Config) -> Result<(), ConfigError> {
    std::fs::create_dir_all(&config.workspace).map_err(|source| ConfigError::WriteFile {
        path: config.workspace.clone(),
        source,
    })?;

    for dir in [
        "skills",
        "agents",
        ".cache",
        "notes",
        "references",
        "diary",
        "projects",
        "shell",
        "feedback",
        "scripts",
        "code",
        "services",
        ".tool-overflow",
    ] {
        let path = config.workspace.join(dir);
        std::fs::create_dir_all(&path).map_err(|source| ConfigError::WriteFile { path, source })?;
    }

    // User-only files (not bundled — never overwritten)
    create_file_if_missing(&config.workspace.join("SOUL.md"), "")?;
    create_file_if_missing(&config.workspace.join("OPERATOR.md"), "")?;

    Ok(())
}

/// Phase 2: Non-destructive install of bundled files.
///
/// Installs new files and auto-updates files the user hasn't touched.
/// Clean merges are applied silently. Conflicts and modified removals
/// are left for the daemon boot to handle via Discord.
pub fn install_bundled_files(config: &Config) -> Result<(), ConfigError> {
    let changes = crate::bundled::compute_changes(&config.workspace);
    crate::bundled::apply_silent_updates(&config.workspace, &changes).map_err(|source| {
        ConfigError::WriteFile {
            path: config.workspace.clone(),
            source,
        }
    })
}

/// Full workspace setup: create directories + non-destructive file install.
/// Used by onboarding and tests.
#[tracing::instrument(skip_all, fields(workspace = %config.workspace.display()))]
pub fn bootstrap_workspace(config: &Config) -> Result<(), ConfigError> {
    bootstrap_workspace_dirs(config)?;
    install_bundled_files(config)
}

fn create_file_if_missing(path: &Path, content: &str) -> Result<(), ConfigError> {
    if path.exists() {
        return Ok(());
    }
    std::fs::write(path, content).map_err(|source| ConfigError::WriteFile {
        path: path.to_path_buf(),
        source,
    })
}
