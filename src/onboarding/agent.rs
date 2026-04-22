use std::collections::BTreeMap;
use std::sync::Arc;

use super::{OnboardingError, OnboardingState, SearchChoice, ServiceChoice};
use crate::config::ProviderKind;
use crate::providers::types::{
    ChatMessage, ChatRequest, ContentBlock, Provider, Role, create_provider,
};

/// Max tokens for onboarding provider validation requests.
const ONBOARDING_MAX_TOKENS: u32 = 1024;

/// Render a human-readable label for a `ServiceChoice`.
fn service_label(choice: &ServiceChoice) -> String {
    match choice {
        ServiceChoice::NixNative => "nix (local)".to_string(),
        ServiceChoice::Native => "native (uv script)".to_string(),
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
        .map(|p| p.as_str().to_string())
        .unwrap_or_else(|| "not set".to_string());

    let model = state.model.as_deref().unwrap_or("not set").to_string();

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

/// Onboarding assistant — uses the real provider abstraction.
pub struct OnboardingAgent {
    provider: Arc<dyn Provider>,
    model: String,
}

impl std::fmt::Debug for OnboardingAgent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OnboardingAgent")
            .field("model", &self.model)
            .finish_non_exhaustive()
    }
}

impl OnboardingAgent {
    /// Create an agent backed by the same provider implementation the daemon uses.
    pub fn new(kind: ProviderKind, model: &str) -> Result<Self, OnboardingError> {
        let provider = create_provider(kind, BTreeMap::new(), None, None, None, None)?;
        Ok(Self {
            provider,
            model: model.to_string(),
        })
    }

    /// Send a single-turn chat and return the assistant's reply text.
    pub async fn chat(
        &self,
        state_summary: &str,
        user_input: &str,
    ) -> Result<String, OnboardingError> {
        let system_prompt = include_str!("../../assets/onboarding-agent-prompt.md");
        let full_system = format!("{system_prompt}{state_summary}");

        let request = ChatRequest {
            model: self.model.clone(),
            messages: vec![
                ChatMessage {
                    role: Role::System,
                    content: vec![ContentBlock::Text { text: full_system }],
                },
                ChatMessage {
                    role: Role::User,
                    content: vec![ContentBlock::Text {
                        text: user_input.to_string(),
                    }],
                },
            ],
            max_tokens: Some(ONBOARDING_MAX_TOKENS),
            ..Default::default()
        };

        let response = self
            .provider
            .chat(request)
            .await
            .map_err(|e| OnboardingError::ProviderValidation(e.to_string()))?;

        // Extract the first text block from the response.
        response
            .content
            .iter()
            .find_map(|block| match block {
                ContentBlock::Text { text } => Some(text.clone()),
                _ => None,
            })
            .ok_or_else(|| {
                OnboardingError::ProviderValidation("empty response from assistant".into())
            })
    }
}

/// Run an interactive help session with the onboarding assistant.
///
/// Loops until the user types "q" or submits an empty string, then returns
/// control to the caller (wizard phase).
pub async fn run_agent_session(
    agent: &OnboardingAgent,
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

        match agent.chat(&state_summary, &input).await {
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
            provider: Some(ProviderKind::OpenRouter),
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
