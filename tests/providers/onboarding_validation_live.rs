//! Live tests for onboarding provider validation.
//!
//! These verify that `validate_provider` produces a successful response from
//! each provider's real API — the same call `ghost init` makes.
//!
//! Run individual providers:
//!   cargo test --features live-tests-llms onboarding_validation_openrouter -- --nocapture
//!   cargo test --features live-tests-llms onboarding_validation_codex -- --nocapture
//!   cargo test --features live-tests-llms onboarding_validation_anthropic -- --nocapture
//!   cargo test --features live-tests-llms onboarding_validation_kimi -- --nocapture

use ghost::config::ProviderKind;
use ghost::onboarding::provider::validate_provider;

/// OpenRouter: requires OPENROUTER_API_KEY env var.
#[tokio::test]
async fn onboarding_validation_openrouter() {
    let _obs = ghost::observability::init_for_live_tests().expect("init observability");

    if std::env::var("OPENROUTER_API_KEY").is_err() {
        eprintln!("OPENROUTER_API_KEY not set, skipping");
        return;
    }

    let model = std::env::var("OPENROUTER_TEST_MODEL")
        .unwrap_or_else(|_| "anthropic/claude-sonnet-4".to_string());

    let result = validate_provider(&ProviderKind::OpenRouter, &model).await;
    assert!(
        result.is_ok(),
        "OpenRouter validation failed: {:#}",
        result.unwrap_err()
    );
}

/// OpenAI OAuth (Codex): requires existing OAuth tokens.
#[tokio::test]
async fn onboarding_validation_codex() {
    let _obs = ghost::observability::init_for_live_tests().expect("init observability");

    if ghost::auth::openai_oauth::auth_status()
        .await
        .expect("read oauth status")
        .is_none()
    {
        eprintln!("No OpenAI OAuth token found; run `ghost auth codex` first. Skipping.");
        return;
    }

    let model = std::env::var("OPENAI_OAUTH_TEST_MODEL")
        .unwrap_or_else(|_| "gpt-5.4".to_string());

    let result = validate_provider(&ProviderKind::OpenAiOAuth, &model).await;
    assert!(
        result.is_ok(),
        "Codex OAuth validation failed: {:#}",
        result.unwrap_err()
    );
}

/// Anthropic: requires Claude Code credentials at ~/.claude/.credentials.json.
#[tokio::test]
async fn onboarding_validation_anthropic() {
    let _obs = ghost::observability::init_for_live_tests().expect("init observability");

    let creds_path = dirs::home_dir().map(|h| h.join(".claude/.credentials.json"));
    if !creds_path.as_ref().is_some_and(|p| p.exists()) {
        eprintln!("No Claude credentials found at ~/.claude/.credentials.json. Skipping.");
        return;
    }

    let model = std::env::var("ANTHROPIC_TEST_MODEL")
        .unwrap_or_else(|_| "claude-sonnet-4-20250514".to_string());

    let result = validate_provider(&ProviderKind::Anthropic, &model).await;
    assert!(
        result.is_ok(),
        "Anthropic validation failed: {:#}",
        result.unwrap_err()
    );
}

/// Kimi: requires KIMI_API_KEY env var.
#[tokio::test]
async fn onboarding_validation_kimi() {
    let _obs = ghost::observability::init_for_live_tests().expect("init observability");

    if std::env::var("KIMI_API_KEY").is_err() {
        eprintln!("KIMI_API_KEY not set, skipping");
        return;
    }

    let model =
        std::env::var("KIMI_TEST_MODEL").unwrap_or_else(|_| "kimi-k2.5".to_string());

    let result = validate_provider(&ProviderKind::Kimi, &model).await;
    assert!(
        result.is_ok(),
        "Kimi validation failed: {:#}",
        result.unwrap_err()
    );
}
