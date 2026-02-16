use std::fs;

use ghost::config::{self, Config};
use ghost::db::{self, GhostDb};
use tempfile::TempDir;

pub fn test_config() -> (Config, TempDir, TempDir) {
    let workspace = TempDir::new().expect("workspace tempdir");
    let config_dir = TempDir::new().expect("config tempdir");

    let config_file = config_dir.path().join("config.toml");
    fs::write(
        &config_file,
        format!(
            "workspace = \"{}\"\n\
\n\
[models]\n\
default = \"primary\"\n\
\n\
[models.primary]\n\
provider = \"openrouter\"\n\
model = \"anthropic/claude-sonnet-4-5-20250929\"\n\
context_window = 200000\n",
            workspace.path().display()
        ),
    )
    .expect("write config file");

    let config = config::load_from_dir(config_dir.path()).expect("load config");
    (config, workspace, config_dir)
}

pub fn test_workspace() -> (Config, TempDir, TempDir) {
    let (config, workspace, config_dir) = test_config();
    config::bootstrap_workspace(&config).expect("bootstrap workspace");
    (config, workspace, config_dir)
}

#[allow(dead_code)]
pub async fn test_database() -> (GhostDb, Config, TempDir, TempDir) {
    let (config, workspace, config_dir) = test_workspace();
    let db = db::connect(&config.workspace)
        .await
        .expect("connect surrealdb");
    (db, config, workspace, config_dir)
}
