use super::OnboardingError;
use crate::config::ProviderKind;

/// Returns the URL where users can browse available models for a provider.
pub fn catalog_url(provider: &ProviderKind) -> &'static str {
    match provider {
        ProviderKind::OpenRouter => "https://openrouter.ai/rankings",
        ProviderKind::Anthropic => "https://docs.anthropic.com/en/docs/about-claude/models",
        ProviderKind::Kimi => "https://kimi.com",
        ProviderKind::OpenAiOAuth => "https://developers.openai.com/codex/models",
    }
}

/// Ask the user to pick an LLM provider, or parse from a CLI flag.
pub fn prompt_provider(
    flag: Option<&str>,
    existing: Option<ProviderKind>,
) -> Result<ProviderKind, OnboardingError> {
    if let Some(value) = flag {
        return ProviderKind::from_cli_flag(value)
            .ok_or_else(|| OnboardingError::InvalidInput(format!("unknown provider: {value}")));
    }

    let default = existing.unwrap_or(ProviderKind::OpenRouter);

    let choice = cliclack::select("Which LLM provider do you want to use?")
        .item(
            ProviderKind::OpenRouter,
            "OpenRouter",
            "recommended — aggregates many models",
        )
        .item(
            ProviderKind::Anthropic,
            "Anthropic (Claude Code OAuth)",
            "uses Claude Code credentials",
        )
        .item(ProviderKind::Kimi, "Kimi", "Moonshot AI")
        .item(
            ProviderKind::OpenAiOAuth,
            "OpenAI (OAuth / Codex)",
            "device-code OAuth flow",
        )
        .initial_value(default)
        .interact()?;

    Ok(choice)
}

/// Collect or verify credentials for the chosen provider.
///
/// Returns `Some(api_key)` for key-based providers (OpenRouter, Kimi),
/// or `None` for OAuth providers (Anthropic, OpenAiOAuth).
pub async fn prompt_credentials(
    provider: &ProviderKind,
    flag: Option<&str>,
    existing: Option<&str>,
) -> Result<Option<String>, OnboardingError> {
    match provider {
        ProviderKind::OpenRouter | ProviderKind::Kimi => {
            let key = match flag {
                Some(k) => k.to_string(),
                None if existing.is_some() => {
                    let keep = cliclack::confirm("Keep existing API key?")
                        .initial_value(true)
                        .interact()?;
                    if keep {
                        existing.unwrap().to_string()
                    } else {
                        cliclack::password(format!("Paste your {} API key:", provider.as_str()))
                            .interact()?
                    }
                }
                None => cliclack::password(format!("Paste your {} API key:", provider.as_str()))
                    .interact()?,
            };
            Ok(Some(key))
        }
        ProviderKind::Anthropic => {
            prompt_anthropic_credentials()?;
            Ok(None)
        }
        ProviderKind::OpenAiOAuth => {
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
    provider: &ProviderKind,
    flag: Option<&str>,
    existing: Option<&str>,
) -> Result<String, OnboardingError> {
    if let Some(m) = flag {
        return Ok(m.to_string());
    }

    let url = catalog_url(provider);
    cliclack::note("Model catalog", format!("Browse models at: {url}"))?;

    let mut input = cliclack::input("Enter the model ID:");
    if let Some(m) = existing {
        input = input.default_input(m);
    } else {
        input = input.placeholder("e.g. anthropic/claude-sonnet-4");
    }
    let model: String = input.interact()?;

    Ok(model)
}

/// Ask the user for the context window size, or accept from a CLI flag.
pub fn prompt_context_window(
    flag: Option<u32>,
    existing: Option<u32>,
) -> Result<u32, OnboardingError> {
    if let Some(v) = flag {
        return Ok(v);
    }

    let default = existing.unwrap_or(200_000).to_string();
    let raw: String = cliclack::input("Context window size (tokens):")
        .default_input(&default)
        .interact()?;

    raw.parse::<u32>().map_err(|_| {
        OnboardingError::InvalidInput(format!("'{raw}' is not a valid context window size"))
    })
}

/// Make a minimal test request to the provider's API to verify credentials.
pub async fn validate_provider(
    provider: &ProviderKind,
    model: &str,
) -> Result<(), OnboardingError> {
    use crate::providers::types::{ChatMessage, ChatRequest, ContentBlock, Role, create_provider};
    use std::collections::BTreeMap;

    let p = create_provider(*provider, BTreeMap::new(), None)?;
    let request = ChatRequest {
        model: model.to_string(),
        system: Some("Reply with OK".to_string()),
        messages: vec![ChatMessage {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: "ping".to_string(),
            }],
        }],
        max_tokens: Some(5),
        ..Default::default()
    };
    p.chat(request)
        .await
        .map_err(|e| OnboardingError::ProviderValidation(e.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_provider_kind_from_flag() {
        assert!(matches!(
            ProviderKind::from_cli_flag("openrouter"),
            Some(ProviderKind::OpenRouter)
        ));
        assert!(matches!(
            ProviderKind::from_cli_flag("anthropic"),
            Some(ProviderKind::Anthropic)
        ));
        assert!(matches!(
            ProviderKind::from_cli_flag("openai-oauth"),
            Some(ProviderKind::OpenAiOAuth)
        ));
        assert!(matches!(
            ProviderKind::from_cli_flag("chatgpt-oauth"),
            Some(ProviderKind::OpenAiOAuth)
        ));
        assert!(ProviderKind::from_cli_flag("invalid").is_none());
    }

    #[test]
    fn provider_config_string_matches_config_rs() {
        assert_eq!(ProviderKind::OpenRouter.as_str(), "openrouter");
        assert_eq!(ProviderKind::Kimi.as_str(), "kimi_code");
        assert_eq!(ProviderKind::OpenAiOAuth.as_str(), "openai_oauth");
        assert_eq!(ProviderKind::Anthropic.as_str(), "anthropic");
    }

    #[test]
    fn catalog_url_per_provider() {
        assert!(catalog_url(&ProviderKind::OpenRouter).contains("openrouter.ai"));
        assert!(catalog_url(&ProviderKind::Kimi).contains("kimi.com"));
        assert!(catalog_url(&ProviderKind::Anthropic).contains("anthropic.com"));
        assert!(catalog_url(&ProviderKind::OpenAiOAuth).contains("developers.openai.com"));
    }
}
