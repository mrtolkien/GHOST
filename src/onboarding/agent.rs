use std::time::Duration;

use super::{OnboardingError, OnboardingState, ProviderChoice, SearchChoice, ServiceChoice};

/// Render a human-readable label for a `ServiceChoice`.
fn service_label(choice: &ServiceChoice) -> String {
    match choice {
        ServiceChoice::NixNative => "nix (local)".to_string(),
        ServiceChoice::Container => "container".to_string(),
        ServiceChoice::Remote(url) => format!("remote ({url})"),
        ServiceChoice::Skip => "skipped".to_string(),
    }
}

/// Render a human-readable label for a `SearchChoice`.
fn search_label(choice: &SearchChoice) -> String {
    match choice {
        SearchChoice::SearxngLocal => "searxng (local container)".to_string(),
        SearchChoice::BraveApi(_) => "brave search api".to_string(),
        SearchChoice::SearxngRemote(url) => format!("searxng remote ({url})"),
        SearchChoice::Skip => "skipped".to_string(),
    }
}

/// Format current wizard state as a text block for the assistant's context.
pub fn build_state_summary(state: &OnboardingState, phase: &str) -> String {
    let provider = state
        .provider
        .as_ref()
        .map(|p| p.as_config_str().to_string())
        .unwrap_or_else(|| "not set".to_string());

    let model = state
        .model
        .as_deref()
        .unwrap_or("not set")
        .to_string();

    let discord = if state.discord_token.is_some() {
        "configured".to_string()
    } else {
        "not set".to_string()
    };

    let embeddings = state
        .embeddings
        .as_ref()
        .map(service_label)
        .unwrap_or_else(|| "not set".to_string());

    let search = state
        .search
        .as_ref()
        .map(search_label)
        .unwrap_or_else(|| "not set".to_string());

    let crawl = state
        .crawl
        .as_ref()
        .map(service_label)
        .unwrap_or_else(|| "not set".to_string());

    let docling = state
        .docling
        .as_ref()
        .map(service_label)
        .unwrap_or_else(|| "not set".to_string());

    format!(
        "\n## Current State\n\
         - Phase: {phase}\n\
         - Provider: {provider}\n\
         - Model: {model}\n\
         - Discord: {discord}\n\
         - Embeddings: {embeddings}\n\
         - Search: {search}\n\
         - Crawl: {crawl}\n\
         - Docling: {docling}\n"
    )
}

/// Thin wrapper around a chat completion endpoint used during onboarding.
#[derive(Debug)]
pub struct OnboardingAgent {
    api_url: String,
    api_key: Option<String>,
    model: String,
}

impl OnboardingAgent {
    /// Build an agent pointing at the given provider's chat completion endpoint.
    pub fn new(provider: &ProviderChoice, api_key: Option<&str>, model: &str) -> Self {
        let api_url = match provider {
            ProviderChoice::OpenRouter => {
                "https://openrouter.ai/api/v1/chat/completions".to_string()
            }
            ProviderChoice::Anthropic => "https://api.anthropic.com/v1/messages".to_string(),
            ProviderChoice::Kimi => {
                "https://api.kimi.com/coding/v1/chat/completions".to_string()
            }
            ProviderChoice::OpenAiOAuth => {
                "https://api.openai.com/v1/chat/completions".to_string()
            }
        };

        Self {
            api_url,
            api_key: api_key.map(|s| s.to_string()),
            model: model.to_string(),
        }
    }

    /// Send a single-turn chat request and return the assistant's reply.
    ///
    /// Uses `tokio::runtime::Handle` to block on the async HTTP call from a
    /// sync context — the wizard loop is synchronous (`cliclack::input`).
    pub fn chat(
        &self,
        provider: &ProviderChoice,
        state_summary: &str,
        user_input: &str,
    ) -> Result<String, OnboardingError> {
        let system_prompt = include_str!("../../assets/onboarding-agent-prompt.md");
        let full_system = format!("{system_prompt}{state_summary}");

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| OnboardingError::InvalidInput(format!("failed to build runtime: {e}")))?;

        rt.block_on(self.chat_async(provider, &full_system, user_input))
    }

    async fn chat_async(
        &self,
        provider: &ProviderChoice,
        system: &str,
        user_input: &str,
    ) -> Result<String, OnboardingError> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()?;

        match provider {
            ProviderChoice::Anthropic => {
                self.chat_anthropic(&client, system, user_input).await
            }
            _ => self.chat_openai_compat(&client, system, user_input).await,
        }
    }

    /// OpenAI-compatible chat completion (OpenRouter, Kimi, OpenAI OAuth).
    async fn chat_openai_compat(
        &self,
        client: &reqwest::Client,
        system: &str,
        user_input: &str,
    ) -> Result<String, OnboardingError> {
        let body = serde_json::json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user_input}
            ]
        });

        let mut req = client.post(&self.api_url).json(&body);
        if let Some(key) = &self.api_key {
            req = req.bearer_auth(key);
        }

        let resp = req.send().await?;
        let status = resp.status();
        let text = resp.text().await?;

        if !status.is_success() {
            return Err(OnboardingError::ProviderValidation(format!(
                "HTTP {status}: {text}"
            )));
        }

        let doc: serde_json::Value = serde_json::from_str(&text).map_err(|e| {
            OnboardingError::InvalidInput(format!("failed to parse response JSON: {e}"))
        })?;

        doc["choices"][0]["message"]["content"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| {
                OnboardingError::InvalidInput(format!(
                    "unexpected response shape (no choices[0].message.content): {text}"
                ))
            })
    }

    /// Anthropic Messages API chat completion.
    async fn chat_anthropic(
        &self,
        client: &reqwest::Client,
        system: &str,
        user_input: &str,
    ) -> Result<String, OnboardingError> {
        let access_token = load_anthropic_access_token()?;

        let body = serde_json::json!({
            "model": self.model,
            "max_tokens": 1024,
            "system": system,
            "messages": [
                {"role": "user", "content": user_input}
            ]
        });

        let resp = client
            .post(&self.api_url)
            .header("x-api-key", &access_token)
            .header("anthropic-version", "2023-06-01")
            .json(&body)
            .send()
            .await?;

        let status = resp.status();
        let text = resp.text().await?;

        if !status.is_success() {
            return Err(OnboardingError::ProviderValidation(format!(
                "HTTP {status}: {text}"
            )));
        }

        let doc: serde_json::Value = serde_json::from_str(&text).map_err(|e| {
            OnboardingError::InvalidInput(format!("failed to parse response JSON: {e}"))
        })?;

        doc["content"][0]["text"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| {
                OnboardingError::InvalidInput(format!(
                    "unexpected response shape (no content[0].text): {text}"
                ))
            })
    }
}

/// Load the Anthropic access token from `~/.claude/.credentials.json`.
fn load_anthropic_access_token() -> Result<String, OnboardingError> {
    let path = dirs::home_dir()
        .map(|h| h.join(".claude/.credentials.json"))
        .ok_or_else(|| OnboardingError::InvalidInput("cannot determine home directory".into()))?;

    let content = std::fs::read_to_string(&path).map_err(|e| {
        OnboardingError::InvalidInput(format!("failed to read {}: {e}", path.display()))
    })?;

    let doc: serde_json::Value = serde_json::from_str(&content).map_err(|e| {
        OnboardingError::InvalidInput(format!("failed to parse credentials: {e}"))
    })?;

    doc.get("claudeAiOauth")
        .and_then(|o| o.get("accessToken"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| {
            OnboardingError::InvalidInput(
                "no claudeAiOauth.accessToken in credentials file".into(),
            )
        })
}

/// Run an interactive help session with the onboarding assistant.
///
/// Loops until the user types "q" or submits an empty string, then returns
/// control to the caller (wizard phase).
pub fn run_agent_session(
    agent: &OnboardingAgent,
    provider: &ProviderChoice,
    state: &OnboardingState,
    phase: &str,
) -> Result<(), OnboardingError> {
    let state_summary = build_state_summary(state, phase);

    let _ = cliclack::log::step("── Onboarding Assistant ──");

    loop {
        let input: String = cliclack::input("You")
            .placeholder("Type your question, or 'q' to return")
            .interact()?;

        if input.trim().is_empty() || input.trim() == "q" {
            break;
        }

        match agent.chat(provider, &state_summary, &input) {
            Ok(reply) => {
                let _ = cliclack::log::info(reply);
            }
            Err(e) => {
                let _ = cliclack::log::warning(format!("Assistant error: {e}"));
            }
        }
    }

    let _ = cliclack::log::step("── Returning to setup ──");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::onboarding::*;

    #[test]
    fn state_summary_includes_configured_fields() {
        let state = OnboardingState {
            provider: Some(ProviderChoice::OpenRouter),
            model: Some("test-model".into()),
            ..Default::default()
        };
        let summary = build_state_summary(&state, "Services");
        assert!(summary.contains("openrouter"));
        assert!(summary.contains("test-model"));
        assert!(summary.contains("Services"));
    }

    #[test]
    fn state_summary_shows_not_set() {
        let state = OnboardingState::default();
        let summary = build_state_summary(&state, "Provider");
        assert!(summary.contains("not set"));
        assert!(summary.contains("Provider"));
    }
}
