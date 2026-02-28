use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::{Config, ModelConfig};

/// Reasoning effort level for models that support it.
///
/// Resolution order (first `Some` wins):
/// `ChatRequest.reasoning_effort` > `TaskDefinition.reasoning_effort`
/// > `ModelConfig.reasoning_effort` > default (`Medium`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningEffort {
    None,
    Low,
    #[default]
    Medium,
    High,
}

impl ReasoningEffort {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

/// Resolve reasoning effort from the three-layer cascade.
#[must_use]
pub fn resolve_reasoning_effort(
    request: Option<ReasoningEffort>,
    task: Option<ReasoningEffort>,
    model: Option<ReasoningEffort>,
) -> ReasoningEffort {
    request.or(task).or(model).unwrap_or_default()
}

#[derive(Debug, Clone, Default)]
pub struct DebugContext {
    pub session_id: String,
    pub iteration: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChatRequest {
    pub model: String,
    #[serde(default)]
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolDefinition>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    /// Reasoning effort level for this request. Providers map this to
    /// their native format (e.g. OpenRouter `reasoning_effort`, Codex
    /// `reasoning.effort`).
    #[serde(skip)]
    pub reasoning_effort: Option<ReasoningEffort>,
    /// Stable identifier for prompt cache routing. Providers that support
    /// prompt caching (e.g. OpenAI Codex) use this as `prompt_cache_key`
    /// to steer requests with the same prefix to the same server.
    #[serde(skip)]
    pub cache_key: String,
    /// Opaque sticky-routing token from the previous response. Providers
    /// that support it (Codex backend) echo this as the
    /// `x-codex-turn-state` request header so the load-balancer routes
    /// consecutive requests to the same server, maximizing cache hits.
    #[serde(skip)]
    pub turn_state: Option<String>,
    #[serde(skip)]
    pub debug_context: Option<DebugContext>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: Role,
    #[serde(default)]
    pub content: Vec<ContentBlock>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    User,
    Assistant,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
        is_error: bool,
    },
    /// Opaque provider output item preserved for faithful echo-back.
    /// `original_type` is the item's native type (e.g. "reasoning").
    /// `value` is the complete raw JSON from the provider.
    /// Tool loop ignores these — they flow through history untouched.
    RawOutput {
        original_type: String,
        value: Value,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    pub content: Vec<ContentBlock>,
    pub usage: Usage,
    pub stop_reason: StopReason,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_id: Option<String>,
    /// Sticky-routing token received from the server. The tool loop
    /// carries this into the next `ChatRequest.turn_state` so the
    /// provider can echo it back, keeping requests on the same server
    /// for prompt cache locality.
    #[serde(skip)]
    pub turn_state: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cache_read_tokens: Option<u32>,
    pub cache_creation_tokens: Option<u32>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    EndTurn,
    ToolUse,
    MaxTokens,
}

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("rate limited (retry after {retry_after_secs:?}s)")]
    RateLimited { retry_after_secs: Option<u64> },

    #[error("authentication failed: {0}")]
    Auth(String),

    #[error("model not found: {0}")]
    ModelNotFound(String),

    #[error("empty response from provider: {detail}")]
    EmptyResponse { detail: String },

    #[error("circuit breaker open for model '{model}' (retry after {retry_after_secs}s)")]
    CircuitOpen {
        model: String,
        retry_after_secs: u64,
    },

    #[error("provider request timed out after {seconds}s")]
    Timeout { seconds: u64 },

    #[error("request failed: {0}")]
    Request(#[from] reqwest::Error),

    #[error("server error (HTTP {status}): {message}")]
    ServerError { status: u16, message: String },

    #[error("invalid response: {0}")]
    InvalidResponse(String),
}

#[derive(Debug, thiserror::Error)]
pub enum ProviderInitError {
    #[error("model alias '{alias}' not found in config")]
    UnknownAlias { alias: String },

    #[error("provider '{provider}' is not supported yet")]
    UnsupportedProvider { provider: String },

    #[error(transparent)]
    Provider(#[from] ProviderError),
}

#[async_trait]
pub trait Provider: Send + Sync {
    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, ProviderError>;
    fn name(&self) -> &str;
}

#[tracing::instrument(skip_all, fields(alias = alias.unwrap_or(&config.models.default)))]
pub fn model_from_alias<'a>(
    config: &'a Config,
    alias: Option<&'a str>,
) -> Result<(&'a str, &'a ModelConfig), ProviderInitError> {
    let alias = alias.unwrap_or(&config.models.default);
    let model =
        config
            .models
            .aliases
            .get(alias)
            .ok_or_else(|| ProviderInitError::UnknownAlias {
                alias: alias.to_string(),
            })?;

    Ok((alias, model))
}

#[tracing::instrument(skip_all, fields(alias = alias.unwrap_or(&config.models.default)))]
pub fn provider_for_alias(
    config: &Config,
    alias: Option<&str>,
) -> Result<Arc<dyn Provider>, ProviderInitError> {
    let (_alias, model) = model_from_alias(config, alias)?;

    match model.provider.as_str() {
        "openrouter" => {
            let mut provider =
                crate::providers::openrouter::OpenRouterProvider::new(model.headers.clone())?;
            provider.set_debug(config.debug.save_requests, &config.workspace);
            Ok(Arc::new(provider))
        }
        "kimi_code" | "kimi" => {
            let mut provider =
                crate::providers::kimi_code::KimiCodeProvider::new(model.headers.clone())?;
            provider.set_debug(config.debug.save_requests, &config.workspace);
            Ok(Arc::new(provider))
        }
        "openai_oauth" => {
            let mut provider =
                crate::providers::openai_oauth::OpenAiOAuthProvider::new(model.headers.clone())?;
            provider.set_debug(config.debug.save_requests, &config.workspace);
            Ok(Arc::new(provider))
        }
        unsupported => Err(ProviderInitError::UnsupportedProvider {
            provider: unsupported.to_string(),
        }),
    }
}

#[must_use]
pub fn user_message(content: impl Into<String>) -> ChatMessage {
    ChatMessage {
        role: Role::User,
        content: vec![ContentBlock::Text {
            text: content.into(),
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reasoning_effort_default_is_medium() {
        assert_eq!(ReasoningEffort::default(), ReasoningEffort::Medium);
    }

    #[test]
    fn reasoning_effort_as_str() {
        assert_eq!(ReasoningEffort::None.as_str(), "none");
        assert_eq!(ReasoningEffort::Low.as_str(), "low");
        assert_eq!(ReasoningEffort::Medium.as_str(), "medium");
        assert_eq!(ReasoningEffort::High.as_str(), "high");
    }

    #[test]
    fn resolve_request_wins() {
        let result = resolve_reasoning_effort(
            Some(ReasoningEffort::Low),
            Some(ReasoningEffort::High),
            Some(ReasoningEffort::Medium),
        );
        assert_eq!(result, ReasoningEffort::Low);
    }

    #[test]
    fn resolve_task_wins_over_model() {
        let result = resolve_reasoning_effort(
            None,
            Some(ReasoningEffort::High),
            Some(ReasoningEffort::Low),
        );
        assert_eq!(result, ReasoningEffort::High);
    }

    #[test]
    fn resolve_model_wins_over_default() {
        let result = resolve_reasoning_effort(None, None, Some(ReasoningEffort::Low));
        assert_eq!(result, ReasoningEffort::Low);
    }

    #[test]
    fn resolve_falls_back_to_medium() {
        let result = resolve_reasoning_effort(None, None, None);
        assert_eq!(result, ReasoningEffort::Medium);
    }

    #[test]
    fn reasoning_effort_serde_roundtrip() {
        let json = serde_json::to_string(&ReasoningEffort::High).unwrap();
        assert_eq!(json, "\"high\"");
        let parsed: ReasoningEffort = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, ReasoningEffort::High);
    }
}
