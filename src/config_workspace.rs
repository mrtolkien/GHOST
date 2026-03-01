use std::path::Path;

use crate::config::{Config, ConfigError};

const DEFAULT_BOOT_TEMPLATE: &str =
    "# BOOT\n\nYou are a GHOST, a personal AI agent for your OPERATOR.\n";

#[tracing::instrument(skip_all, fields(workspace = %config.workspace.display()))]
pub fn bootstrap_workspace(config: &Config) -> Result<(), ConfigError> {
    std::fs::create_dir_all(&config.workspace).map_err(|source| ConfigError::WriteFile {
        path: config.workspace.clone(),
        source,
    })?;

    for dir in ["skills", "agents", ".cache", "notes", "references", "diary"] {
        let path = config.workspace.join(dir);
        std::fs::create_dir_all(&path).map_err(|source| ConfigError::WriteFile { path, source })?;
    }

    create_file_if_missing(&config.workspace.join("BOOT.md"), DEFAULT_BOOT_TEMPLATE)?;
    create_file_if_missing(&config.workspace.join("SOUL.md"), "")?;
    create_file_if_missing(&config.workspace.join("OPERATOR.md"), "")?;

    crate::skills::install_default_skills(&config.workspace).map_err(|source| {
        ConfigError::WriteFile {
            path: config.workspace.join("skills"),
            source,
        }
    })?;

    crate::agents::install_default_agents(&config.workspace).map_err(|source| {
        ConfigError::WriteFile {
            path: config.workspace.join("agents"),
            source,
        }
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
