mod common;

use std::fs;
use std::sync::{Mutex, OnceLock};

use ghost::config::{self, CONFIG_DIR_ENV};
use ghost::config_cli;
use ghost::providers::ReasoningEffort;
use tempfile::TempDir;

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[test]
fn config_loads_defaults_for_missing_fields() {
    let (config, _workspace, _config_dir) = common::test_config();

    assert_eq!(config.models.default, "primary");
    assert_eq!(config.embeddings.batch_size, 32);
    assert_eq!(config.compaction.threshold, 0.90);
    assert_eq!(config.compaction.keep_window, 20);
}

#[test]
fn ghost_config_dir_env_var_overrides_default() {
    let _guard = env_lock().lock().expect("env lock");

    let workspace = TempDir::new().expect("workspace tempdir");
    let config_dir = TempDir::new().expect("config tempdir");

    fs::write(
        config_dir.path().join("config.toml"),
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

    // SAFETY: test-level synchronization via env_lock prevents concurrent mutation.
    unsafe {
        std::env::set_var(CONFIG_DIR_ENV, config_dir.path());
    }

    let config = config::load().expect("load config from env override");
    assert_eq!(config.workspace, workspace.path());

    // SAFETY: test-level synchronization via env_lock prevents concurrent mutation.
    unsafe {
        std::env::remove_var(CONFIG_DIR_ENV);
    }
}

#[test]
fn config_set_updates_toml_correctly() {
    let config_dir = TempDir::new().expect("config tempdir");
    fs::write(
        config_dir.path().join("config.toml"),
        "[models]\ndefault = \"primary\"\n\n[models.primary]\nprovider = \"openrouter\"\nmodel = \"anthropic/claude-sonnet-4-5-20250929\"\ncontext_window = 200000\n",
    )
    .expect("write config");
    config_cli::set_value_in_dir(config_dir.path(), "workspace", "/custom/path")
        .expect("set workspace");

    let content = fs::read_to_string(config_dir.path().join("config.toml")).expect("read config");
    assert!(content.contains("workspace = \"/custom/path\""));
}

#[test]
fn config_set_rejects_unknown_key_paths() {
    let config_dir = TempDir::new().expect("config tempdir");
    fs::write(
        config_dir.path().join("config.toml"),
        "[models]\ndefault = \"primary\"\n\n[models.primary]\nprovider = \"openrouter\"\nmodel = \"anthropic/claude-sonnet-4-5-20250929\"\ncontext_window = 200000\n",
    )
    .expect("write config");
    let error =
        config_cli::set_value_in_dir(config_dir.path(), "web.unknown", "x").expect_err("must fail");
    let message = error.to_string();

    assert!(message.contains("invalid config in"));
    assert!(message.contains("unknown field"));
    assert!(message.contains("unknown"));
}

#[test]
fn config_set_can_create_new_model_alias_on_first_write() {
    let config_dir = TempDir::new().expect("config tempdir");
    fs::write(
        config_dir.path().join("config.toml"),
        "[models]\ndefault = \"primary\"\n\n[models.primary]\nprovider = \"openrouter\"\nmodel = \"anthropic/claude-sonnet-4-5-20250929\"\ncontext_window = 200000\n",
    )
    .expect("write config");
    config_cli::set_value_in_dir(
        config_dir.path(),
        "models.experimental",
        "{ provider = \"openrouter\", model = \"anthropic/claude-sonnet-4-5-20250929\", context_window = 200000 }",
    )
    .expect("set model object");

    let config = config::load_from_dir(config_dir.path()).expect("load config");
    let model = config
        .models
        .aliases
        .get("experimental")
        .expect("experimental alias exists");

    assert_eq!(model.provider, "openrouter");
    assert_eq!(model.model, "anthropic/claude-sonnet-4-5-20250929");
}

#[test]
fn config_set_model_object_requires_provider_and_model() {
    let config_dir = TempDir::new().expect("config tempdir");
    fs::write(
        config_dir.path().join("config.toml"),
        "[models]\ndefault = \"primary\"\n\n[models.primary]\nprovider = \"openrouter\"\nmodel = \"anthropic/claude-sonnet-4-5-20250929\"\ncontext_window = 200000\n",
    )
    .expect("write config");
    let error = config_cli::set_value_in_dir(
        config_dir.path(),
        "models.partial",
        "{ provider = \"openrouter\" }",
    )
    .expect_err("must fail");
    let message = error.to_string();

    assert!(message.contains("requires both provider and model"));
}

#[test]
fn config_set_model_provider_must_be_valid() {
    let config_dir = TempDir::new().expect("config tempdir");
    fs::write(
        config_dir.path().join("config.toml"),
        "[models]\ndefault = \"primary\"\n\n[models.primary]\nprovider = \"openrouter\"\nmodel = \"anthropic/claude-sonnet-4-5-20250929\"\ncontext_window = 200000\n",
    )
    .expect("write config");
    let error = config_cli::set_value_in_dir(
        config_dir.path(),
        "models.bad",
        "{ provider = \"invalid\", model = \"foo/bar\", context_window = 200000 }",
    )
    .expect_err("must fail");
    let message = error.to_string();

    assert!(message.contains("invalid config in"));
    assert!(message.contains("invalid"));
}

#[test]
fn config_get_prints_resolved_default_alias() {
    let (_config, _workspace, config_dir) = common::test_config();
    let value = config_cli::get_resolved_value_from_dir(config_dir.path(), "models.default")
        .expect("get default model alias");

    assert_eq!(value, "primary");
}

#[test]
fn workspace_bootstrap_creates_identity_files() {
    let (_config, workspace, _config_dir) = common::test_workspace();

    assert!(workspace.path().join("BOOT.md").exists());
    assert!(workspace.path().join("SOUL.md").exists());
    assert!(workspace.path().join("OPERATOR.md").exists());
    assert!(workspace.path().join("skills").exists());
    assert!(workspace.path().join("agents").exists());
    assert!(workspace.path().join(".cache").exists());
    assert!(workspace.path().join("notes").exists());

    // Default agents installed as Lua folders
    assert!(
        workspace
            .path()
            .join("agents/deep-research/agent.lua")
            .exists()
    );
    assert!(
        workspace
            .path()
            .join("agents/deep-research/prompt.md")
            .exists()
    );
    assert!(
        workspace
            .path()
            .join("agents/deep-research-reflection/agent.lua")
            .exists()
    );
    assert!(
        workspace
            .path()
            .join("agents/chat-reflection/agent.lua")
            .exists()
    );
    assert!(
        workspace.path().join("agents/crontab.lua").exists(),
        "crontab.lua should be installed by default"
    );
}

#[test]
fn config_parses_reasoning_effort_on_model() {
    let config_dir = TempDir::new().expect("config tempdir");
    fs::write(
        config_dir.path().join("config.toml"),
        "[models]\ndefault = \"primary\"\n\n\
         [models.primary]\n\
         provider = \"openrouter\"\n\
         model = \"test/model\"\n\
         context_window = 200000\n\
         reasoning_effort = \"low\"\n",
    )
    .expect("write config");

    let config = config::load_from_dir(config_dir.path()).expect("load config");
    let model = config.models.aliases.get("primary").expect("primary alias");
    assert_eq!(model.reasoning_effort, Some(ReasoningEffort::Low));
}

#[test]
fn config_reasoning_effort_defaults_to_none() {
    let config_dir = TempDir::new().expect("config tempdir");
    fs::write(
        config_dir.path().join("config.toml"),
        "[models]\ndefault = \"primary\"\n\n\
         [models.primary]\n\
         provider = \"openrouter\"\n\
         model = \"test/model\"\n\
         context_window = 200000\n",
    )
    .expect("write config");

    let config = config::load_from_dir(config_dir.path()).expect("load config");
    let model = config.models.aliases.get("primary").expect("primary alias");
    assert!(model.reasoning_effort.is_none());
}

#[test]
fn invalid_config_contains_file_path_and_field_name() {
    let config_dir = TempDir::new().expect("config tempdir");
    let config_path = config_dir.path().join("config.toml");

    fs::write(&config_path, "workspace = \"~/GHOST\"\nunknown = true\n")
        .expect("write invalid config");

    let error = config::load_from_dir(config_dir.path()).expect_err("expected parse error");
    let message = error.to_string();

    assert!(message.contains(&config_path.display().to_string()));
    assert!(message.contains("unknown"));
}
