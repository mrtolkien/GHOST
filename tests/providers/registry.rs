use std::fs;

use ghost::config::{self, ProviderKind};
use ghost::providers::{model_from_alias, provider_for_alias, types::user_message};
use tempfile::TempDir;

use super::common;

#[test]
fn resolves_default_model_alias() {
    let (config, _workspace, _config_dir) = common::test_config();
    let (alias, model) = model_from_alias(&config, None).expect("resolve default alias");

    assert_eq!(alias, "primary");
    assert_eq!(model.provider, ProviderKind::OpenRouter);
}

#[test]
fn unknown_alias_returns_error() {
    let (config, _workspace, _config_dir) = common::test_config();
    let error = model_from_alias(&config, Some("missing")).expect_err("must fail");
    assert!(matches!(
        error,
        ghost::providers::ProviderInitError::UnknownAlias { .. }
    ));
}

#[test]
fn request_helper_creates_user_message() {
    let msg = user_message("hello");
    assert_eq!(msg.role, ghost::providers::Role::User);
    assert_eq!(msg.content.len(), 1);
}

#[test]
fn openai_compatible_provider_initializes_without_auth_env() {
    let workspace = TempDir::new().expect("workspace tempdir");
    let config_dir = TempDir::new().expect("config tempdir");
    fs::write(
        config_dir.path().join("config.toml"),
        format!(
            "workspace = \"{}\"\n\
\n\
[models]\n\
default = \"local\"\n\
\n\
[models.local]\n\
provider = \"openai_compatible\"\n\
model = \"gemma4:26b\"\n\
context_window = 131072\n\
base_url = \"http://127.0.0.1:11434/v1/chat/completions\"\n",
            workspace.path().display()
        ),
    )
    .expect("write config");

    let config = config::load_from_dir(config_dir.path()).expect("load config");
    let provider = provider_for_alias(&config, Some("local")).expect("init provider");

    assert_eq!(provider.name(), "openai_compatible");
}
