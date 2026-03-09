use std::path::Path;

use crate::config::{Config, ConfigError};

#[tracing::instrument(skip_all, fields(workspace = %config.workspace.display()))]
pub fn bootstrap_workspace(config: &Config) -> Result<(), ConfigError> {
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
    ] {
        let path = config.workspace.join(dir);
        std::fs::create_dir_all(&path).map_err(|source| ConfigError::WriteFile { path, source })?;
    }

    // User-only files (not bundled — never overwritten)
    create_file_if_missing(&config.workspace.join("SOUL.md"), "")?;
    create_file_if_missing(&config.workspace.join("OPERATOR.md"), "")?;

    // All bundled files (skills, agents, flake, etc.)
    crate::bundled::install_all(&config.workspace).map_err(|source| ConfigError::WriteFile {
        path: config.workspace.clone(),
        source,
    })?;

    Ok(())
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
