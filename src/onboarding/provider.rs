use std::time::Duration;

use super::{OnboardingError, ProviderChoice};

/// Returns the URL where users can browse available models for a provider.
pub fn catalog_url(provider: &ProviderChoice) -> &'static str {
    match provider {
        ProviderChoice::OpenRouter => "https://openrouter.ai/rankings",
        ProviderChoice::Anthropic => "https://docs.anthropic.com/en/docs/about-claude/models",
        ProviderChoice::Kimi => "https://kimi.com",
        ProviderChoice::OpenAiOAuth => "https://developers.openai.com/codex/models",
    }
}

/// Ask the user to pick an LLM provider, or parse from a CLI flag.
pub fn prompt_provider(flag: Option<&str>) -> Result<ProviderChoice, OnboardingError> {
    if let Some(value) = flag {
        return ProviderChoice::from_flag(value);
    }

    let choice = cliclack::select("Which LLM provider do you want to use?")
        .item(
            ProviderChoice::OpenRouter,
            "OpenRouter",
            "recommended — aggregates many models",
        )
        .item(
            ProviderChoice::Anthropic,
            "Anthropic (Claude Code OAuth)",
            "uses Claude Code credentials",
        )
        .item(ProviderChoice::Kimi, "Kimi", "Moonshot AI")
        .item(
            ProviderChoice::OpenAiOAuth,
            "OpenAI (OAuth / Codex)",
            "device-code OAuth flow",
        )
        .interact()?;

    Ok(choice)
}

/// Collect or verify credentials for the chosen provider.
///
/// Returns `Some(api_key)` for key-based providers (OpenRouter, Kimi),
/// or `None` for OAuth providers (Anthropic, OpenAiOAuth).
pub async fn prompt_credentials(
    provider: &ProviderChoice,
    flag: Option<&str>,
) -> Result<Option<String>, OnboardingError> {
    match provider {
        ProviderChoice::OpenRouter | ProviderChoice::Kimi => {
            let key = match flag {
                Some(k) => k.to_string(),
                None => {
                    cliclack::password(format!("Paste your {} API key:", provider.as_config_str()))
                        .interact()?
                }
            };
            Ok(Some(key))
        }
        ProviderChoice::Anthropic => {
            prompt_anthropic_credentials()?;
            Ok(None)
        }
        ProviderChoice::OpenAiOAuth => {
            prompt_openai_oauth_credentials().await?;
            Ok(None)
        }
    }
}

/// Check for existing Anthropic (Claude Code) credentials on disk.
fn prompt_anthropic_credentials() -> Result<(), OnboardingError> {
    let path = dirs::home_dir().map(|h| h.join(".claude/.credentials.json"));

    match path {
        Some(p) if p.exists() => {
            let _ = cliclack::log::success(format!("Claude credentials found at {}", p.display()));
            Ok(())
        }
        _ => Err(OnboardingError::ProviderValidation(
            "Claude credentials not found at ~/.claude/.credentials.json. \
             Install and run `claude` (Claude Code) first to authenticate."
                .to_string(),
        )),
    }
}

/// Check for existing OpenAI OAuth tokens or run the device-code flow.
async fn prompt_openai_oauth_credentials() -> Result<(), OnboardingError> {
    let existing = crate::auth::openai_oauth::auth_status()
        .await
        .map_err(|e| {
            OnboardingError::ProviderValidation(format!("failed to check OpenAI OAuth status: {e}"))
        })?;

    if existing.is_some() {
        let _ = cliclack::log::success("OpenAI OAuth tokens found");
        return Ok(());
    }

    let _ = cliclack::log::info("No OpenAI OAuth tokens found — starting device-code flow...");

    crate::auth::openai_oauth::run_codex_auth_flow()
        .await
        .map_err(|e| {
            OnboardingError::ProviderValidation(format!("OpenAI OAuth flow failed: {e}"))
        })?;

    let _ = cliclack::log::success("OpenAI OAuth tokens saved");
    Ok(())
}

/// Ask the user for a model ID, or accept from a CLI flag.
pub fn prompt_model(
    provider: &ProviderChoice,
    flag: Option<&str>,
) -> Result<String, OnboardingError> {
    if let Some(m) = flag {
        return Ok(m.to_string());
    }

    let url = catalog_url(provider);
    cliclack::note("Model catalog", format!("Browse models at: {url}"))?;

    let model: String = cliclack::input("Enter the model ID:")
        .placeholder("e.g. anthropic/claude-sonnet-4")
        .interact()?;

    Ok(model)
}

/// Ask the user for the context window size, or accept from a CLI flag.
pub fn prompt_context_window(flag: Option<u32>) -> Result<u32, OnboardingError> {
    if let Some(v) = flag {
        return Ok(v);
    }

    let raw: String = cliclack::input("Context window size (tokens):")
        .default_input("200000")
        .interact()?;

    raw.parse::<u32>().map_err(|_| {
        OnboardingError::InvalidInput(format!("'{raw}' is not a valid context window size"))
    })
}

/// Make a minimal test request to the provider's API to verify credentials.
pub async fn validate_provider(
    provider: &ProviderChoice,
    api_key: Option<&str>,
    model: &str,
) -> Result<(), OnboardingError> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()?;

    let result = match provider {
        ProviderChoice::OpenRouter => validate_openrouter(&client, api_key, model).await,
        ProviderChoice::Kimi => validate_kimi(&client, api_key, model).await,
        ProviderChoice::Anthropic => validate_anthropic(&client, model).await,
        ProviderChoice::OpenAiOAuth => validate_openai_oauth(&client, model).await,
    };

    result.map_err(|msg| OnboardingError::ProviderValidation(msg))
}

/// Standard OpenAI-compatible chat completion body.
fn chat_completion_body(model: &str) -> serde_json::Value {
    serde_json::json!({
        "model": model,
        "max_tokens": 5,
        "messages": [
            {"role": "system", "content": "Reply with OK"},
            {"role": "user", "content": "ping"}
        ]
    })
}

async fn validate_openrouter(
    client: &reqwest::Client,
    api_key: Option<&str>,
    model: &str,
) -> Result<(), String> {
    let key = api_key.ok_or("OpenRouter requires an API key")?;
    let resp = client
        .post("https://openrouter.ai/api/v1/chat/completions")
        .bearer_auth(key)
        .json(&chat_completion_body(model))
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;

    check_response(resp).await
}

async fn validate_kimi(
    client: &reqwest::Client,
    api_key: Option<&str>,
    model: &str,
) -> Result<(), String> {
    let key = api_key.ok_or("Kimi requires an API key")?;
    let resp = client
        .post("https://api.kimi.com/coding/v1/chat/completions")
        .bearer_auth(key)
        .json(&chat_completion_body(model))
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;

    check_response(resp).await
}

async fn validate_anthropic(client: &reqwest::Client, model: &str) -> Result<(), String> {
    let access_token = load_anthropic_access_token()?;

    let body = serde_json::json!({
        "model": model,
        "max_tokens": 5,
        "messages": [
            {"role": "user", "content": "ping"}
        ],
        "system": "Reply with OK"
    });

    let resp = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", &access_token)
        .header("anthropic-version", "2023-06-01")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;

    check_response(resp).await
}

/// Read the Anthropic access token from `~/.claude/.credentials.json`.
fn load_anthropic_access_token() -> Result<String, String> {
    let path = dirs::home_dir()
        .map(|h| h.join(".claude/.credentials.json"))
        .ok_or("cannot determine home directory")?;

    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("failed to read {}: {e}", path.display()))?;

    let doc: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| format!("failed to parse credentials: {e}"))?;

    doc.get("claudeAiOauth")
        .and_then(|o| o.get("accessToken"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "no claudeAiOauth.accessToken in credentials file".to_string())
}

async fn validate_openai_oauth(client: &reqwest::Client, model: &str) -> Result<(), String> {
    let store = crate::auth::openai_oauth::TokenStore::default_openai_store()
        .map_err(|e| format!("failed to open token store: {e}"))?;
    let oauth_client = crate::auth::openai_oauth::OpenAiOAuthClient::new()
        .map_err(|e| format!("failed to create OAuth client: {e}"))?;
    let access_token = store
        .get_valid_access_token(&oauth_client)
        .await
        .map_err(|e| format!("failed to get access token: {e}"))?;

    let resp = client
        .post("https://api.openai.com/v1/chat/completions")
        .bearer_auth(&access_token)
        .json(&chat_completion_body(model))
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;

    check_response(resp).await
}

/// Check that an HTTP response indicates success.
async fn check_response(resp: reqwest::Response) -> Result<(), String> {
    let status = resp.status();
    if status.is_success() {
        return Ok(());
    }

    let body = resp
        .text()
        .await
        .unwrap_or_else(|_| "<unreadable body>".to_string());

    Err(format!("HTTP {status}: {body}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::onboarding::ProviderChoice;

    #[test]
    fn parse_provider_choice_from_flag() {
        assert!(matches!(
            ProviderChoice::from_flag("openrouter"),
            Ok(ProviderChoice::OpenRouter)
        ));
        assert!(matches!(
            ProviderChoice::from_flag("anthropic"),
            Ok(ProviderChoice::Anthropic)
        ));
        assert!(matches!(
            ProviderChoice::from_flag("openai-oauth"),
            Ok(ProviderChoice::OpenAiOAuth)
        ));
        assert!(matches!(
            ProviderChoice::from_flag("chatgpt-oauth"),
            Ok(ProviderChoice::OpenAiOAuth)
        ));
        assert!(ProviderChoice::from_flag("invalid").is_err());
    }

    #[test]
    fn provider_config_string_matches_config_rs() {
        assert_eq!(ProviderChoice::OpenRouter.as_config_str(), "openrouter");
        assert_eq!(ProviderChoice::Kimi.as_config_str(), "kimi");
        assert_eq!(ProviderChoice::OpenAiOAuth.as_config_str(), "openai_oauth");
        assert_eq!(ProviderChoice::Anthropic.as_config_str(), "anthropic");
    }

    #[test]
    fn catalog_url_per_provider() {
        assert!(catalog_url(&ProviderChoice::OpenRouter).contains("openrouter.ai"));
        assert!(catalog_url(&ProviderChoice::Kimi).contains("kimi.com"));
        assert!(catalog_url(&ProviderChoice::Anthropic).contains("anthropic.com"));
        assert!(catalog_url(&ProviderChoice::OpenAiOAuth).contains("developers.openai.com"));
    }
}
