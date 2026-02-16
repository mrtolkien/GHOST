mod common;

use ghost::providers::{
    ProviderInitError, model_from_alias, provider_for_alias, types::user_message,
};

#[test]
fn resolves_default_model_alias() {
    let (config, _workspace, _config_dir) = common::test_config();
    let (alias, model) = model_from_alias(&config, None).expect("resolve default alias");

    assert_eq!(alias, "primary");
    assert_eq!(model.provider, "openrouter");
}

#[test]
fn unknown_alias_returns_error() {
    let (config, _workspace, _config_dir) = common::test_config();
    let error = model_from_alias(&config, Some("missing")).expect_err("must fail");
    assert!(matches!(error, ProviderInitError::UnknownAlias { .. }));
}

#[test]
fn unsupported_provider_returns_error() {
    let (mut config, _workspace, _config_dir) = common::test_config();
    let model = config
        .models
        .aliases
        .get_mut("primary")
        .expect("primary model must exist");
    model.provider = "kimi".to_string();

    let error = match provider_for_alias(&config, Some("primary")) {
        Ok(_) => panic!("must fail"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        ProviderInitError::UnsupportedProvider { .. }
    ));
}

#[test]
fn request_helper_creates_user_message() {
    let msg = user_message("hello");
    assert_eq!(msg.role, ghost::providers::Role::User);
    assert_eq!(msg.content.len(), 1);
}
