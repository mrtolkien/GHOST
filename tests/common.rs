use std::fs;
use std::path::PathBuf;

use ghost::config::{self, Config};
use ghost::db::{self, GhostDb};
use ghost::knowledge::{NoteFrontMatter, serialize_note};
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

#[allow(dead_code)]
pub fn write_test_note(workspace: &std::path::Path, title: &str, body: &str) -> PathBuf {
    let front = NoteFrontMatter {
        title: title.to_string(),
        archetype: None,
        tags: vec![],
        trust: 5,
    };
    let content = serialize_note(&front, body).expect("serialize note");
    let slug = ghost::knowledge::slug_from_title(title);
    let path = workspace.join("knowledge/notes").join(format!("{slug}.md"));
    fs::write(&path, content).expect("write test note");
    path
}

#[allow(dead_code)]
pub fn write_test_reference(
    workspace: &std::path::Path,
    topic: &str,
    filename: &str,
    content: &str,
) -> PathBuf {
    let dir = workspace.join("knowledge/references").join(topic);
    fs::create_dir_all(&dir).expect("create reference dir");
    let path = dir.join(filename);
    fs::write(&path, content).expect("write test reference");
    path
}

// ---------------------------------------------------------------------------
// Live test infrastructure
// ---------------------------------------------------------------------------

/// Environment for live e2e tests: fresh temp DB with real provider config.
///
/// On drop, snapshots the workspace to `e2e-output/<timestamp>_<test_name>/`
/// and restores env vars.
#[cfg(feature = "live-tests")]
#[allow(dead_code)]
pub struct LiveTestEnv {
    pub db: GhostDb,
    pub config: Config,
    workspace: TempDir,
    _config_dir: TempDir,
    test_name: String,
    prev_config_dir_env: Option<String>,
    prev_path_env: Option<String>,
}

#[cfg(feature = "live-tests")]
impl Drop for LiveTestEnv {
    fn drop(&mut self) {
        // Snapshot workspace for human validation
        let timestamp = chrono::Utc::now().format("%Y-%m-%dT%H-%M-%S");
        let dest = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("e2e-output")
            .join(format!("{timestamp}_{}", self.test_name));
        if let Err(e) = copy_dir_all(self.workspace.path(), &dest) {
            eprintln!("warning: failed to snapshot workspace: {e}");
        } else {
            eprintln!("e2e snapshot: {}", dest.display());
        }

        // Restore env vars
        match &self.prev_config_dir_env {
            Some(val) => unsafe { std::env::set_var(ghost::config::CONFIG_DIR_ENV, val) },
            None => unsafe { std::env::remove_var(ghost::config::CONFIG_DIR_ENV) },
        }
        if let Some(val) = &self.prev_path_env {
            unsafe { std::env::set_var("PATH", val) };
        }
    }
}

/// Create a live test environment: real provider from `~/.config/ghost/`,
/// fresh temp workspace + database, `GHOST_CONFIG_DIR` set so spawned
/// `ghost` subprocesses use the temp workspace.
#[cfg(feature = "live-tests")]
#[allow(dead_code)]
pub async fn live_test_database(test_name: &str) -> LiveTestEnv {
    let _ = ghost::observability::init_for_live_tests();

    // Save current env state (before we change anything)
    let prev_config_dir = std::env::var(ghost::config::CONFIG_DIR_ENV).ok();
    let prev_path = std::env::var("PATH").ok();

    // Find the real config dir
    let real_config_dir = std::env::var_os(ghost::config::CONFIG_DIR_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = std::env::var("HOME").expect("HOME env var");
            PathBuf::from(home).join(".config/ghost")
        });

    // Create temp workspace + config dir
    let workspace = TempDir::new().expect("workspace tempdir");
    let config_dir = TempDir::new().expect("config tempdir");

    // Read real config.toml, replace workspace path only
    let raw_toml = fs::read_to_string(real_config_dir.join("config.toml"))
        .expect("read real config.toml — is ~/.config/ghost/config.toml present?");
    let mut toml_value: toml::Value = toml::from_str(&raw_toml).expect("parse real config.toml");
    toml_value.as_table_mut().unwrap().insert(
        "workspace".to_string(),
        toml::Value::String(workspace.path().display().to_string()),
    );
    let modified_toml = toml::to_string_pretty(&toml_value).expect("serialize config");
    fs::write(config_dir.path().join("config.toml"), &modified_toml)
        .expect("write temp config.toml");

    // Copy tokens/ and .env from real config dir (OAuth tokens, secrets)
    let tokens_src = real_config_dir.join("tokens");
    if tokens_src.exists() {
        copy_dir_all(&tokens_src, &config_dir.path().join("tokens")).expect("copy tokens dir");
    }
    let env_src = real_config_dir.join(".env");
    if env_src.exists() {
        fs::copy(&env_src, config_dir.path().join(".env")).expect("copy .env");
    }

    // Set env vars so both us and spawned `ghost` processes use the temp config
    unsafe {
        std::env::set_var(ghost::config::CONFIG_DIR_ENV, config_dir.path());

        // Add target/debug to PATH so `ghost web fetch` works in subprocess
        let target_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/debug");
        let path = std::env::var("PATH").unwrap_or_default();
        std::env::set_var("PATH", format!("{}:{}", target_dir.display(), path));
    }

    // Load config from temp dir + bootstrap + connect
    let config = config::load_from_dir(config_dir.path()).expect("load config from temp dir");
    config::bootstrap_workspace(&config).expect("bootstrap temp workspace");
    let db = db::connect(&config.workspace)
        .await
        .expect("connect to fresh temp database");

    LiveTestEnv {
        db,
        config,
        workspace,
        _config_dir: config_dir,
        test_name: test_name.to_string(),
        prev_config_dir_env: prev_config_dir,
        prev_path_env: prev_path,
    }
}

#[cfg(feature = "live-tests")]
fn copy_dir_all(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let dest_path = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_all(&entry.path(), &dest_path)?;
        } else {
            fs::copy(entry.path(), dest_path)?;
        }
    }
    Ok(())
}
