use ghost::config::ProviderKind;
use ghost::providers::{model_from_alias, types::user_message};

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
