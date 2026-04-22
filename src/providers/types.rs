use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::{Config, ModelConfig, ProviderKind};

/// Reasoning effort level for models that support it.
///
/// Resolution order (first `Some` wins):
/// `ChatRequest.reasoning_effort` > `AgentConfig.reasoning_effort`
/// > `ModelConfig.reasoning_effort` > default (`Medium`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningEffort {
    None,
    Low,
    #[default]
    Medium,
    High,
    Xhigh,
}

impl ReasoningEffort {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Xhigh => "xhigh",
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
    /// Codex text output verbosity. Default "low"; override to "medium" for
    /// models like gpt-5-codex that don't support "low".
    #[serde(skip)]
    pub text_verbosity: Option<String>,
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
    /// An image reference. `path` is the absolute filesystem path; base64
    /// encoding happens at provider send time.
    Image {
        path: String,
        mime_type: String,
        filename: String,
    },
    /// Model reasoning/thinking block. Typed for correct ordering and
    /// cross-model transitions.
    ///
    /// - Anthropic `thinking`: text + signature (readable, verifiable)
    /// - Anthropic `redacted_thinking`: opaque_data only
    /// - OpenAI `reasoning`: opaque_data (encrypted) + text (summary)
    Thinking {
        /// Human-readable reasoning text.
        text: Option<String>,
        /// Anthropic thinking signature for round-trip verification.
        signature: Option<String>,
        /// Opaque/encrypted data (Anthropic redacted_thinking `data`,
        /// OpenAI reasoning `encrypted_content`).
        opaque_data: Option<String>,
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

    #[error("context overflow: {0}")]
    ContextOverflow(String),

    #[error(
        "this session contains thinking/reasoning blocks from a different model \
         that are incompatible with the current one. \
         Switch back to the previous model or /reboot to start a new session."
    )]
    IncompatibleHistory(String),

    #[error("invalid response: {0}")]
    InvalidResponse(String),

    #[error("{}", format_chain_exhausted(errors))]
    ChainExhausted {
        errors: Vec<(String, Box<ProviderError>)>,
    },
}

fn format_chain_exhausted(errors: &[(String, Box<ProviderError>)]) -> String {
    let details: Vec<String> = errors
        .iter()
        .map(|(alias, err)| format!("{alias} ({err})"))
        .collect();
    format!("all models in chain failed: {}", details.join(", "))
}

impl ProviderError {
    /// Check whether an error message indicates the request exceeded the
    /// model's context window. Provider-agnostic — covers patterns from
    /// OpenAI, Anthropic, OpenRouter, Gemini, Kimi, and other
    /// OpenAI-compatible APIs.
    pub fn is_context_overflow_message(msg: &str) -> bool {
        let lower = msg.to_lowercase();
        // OpenAI Responses API: "Your input exceeds the context window of this model"
        lower.contains("exceeds the context window")
            // OpenAI Responses API (alt): "the context window of this model"
            || lower.contains("context window of this model")
            // OpenAI Chat Completions / OpenRouter: "This model's maximum context length is N tokens"
            || lower.contains("maximum context length")
            // OpenAI error code (JSON field): "context_length_exceeded"
            || lower.contains("context_length_exceeded")
            // OpenAI error code (in message text): "context length exceeded"
            || lower.contains("context length exceeded")
            // General: "too many tokens"
            || lower.contains("too many tokens")
            // General: "token limit exceeded"
            || lower.contains("token limit exceeded")
            // Anthropic: "prompt is too long: N tokens > M maximum"
            || lower.contains("prompt is too long")
            // General: "input is too long"
            || lower.contains("input is too long")
            // Legacy: "prompt_length exceeded"
            || lower.contains("prompt_length exceeded")
            // OpenAI GPT-5: "Input tokens exceed the configured limit of N"
            || lower.contains("input tokens exceed")
            // Anthropic: "input length and `max_tokens` exceed context limit: N + M > L"
            || lower.contains("exceed context limit")
            // Anthropic 429: "Extra usage is required for long context requests"
            || lower.contains("extra usage is required")
            // Gemini: "The input token count (N) exceeds the maximum number of tokens allowed (M)"
            || lower.contains("exceeds the maximum number of tokens")
            // Gemini Vertex AI: "input token count is N but model only supports up to M"
            || lower.contains("input token count is")
            // Kimi: "Input token length too long"
            || lower.contains("token length too long")
            // Kimi: "Your request exceeded model token limit: N"
            || lower.contains("exceeded model token limit")
    }

    /// Check whether an error indicates incompatible thinking/reasoning
    /// blocks from a different model in the conversation history.
    /// Provider-agnostic — covers Anthropic `redacted_thinking` and
    /// OpenAI/Codex reasoning block errors.
    pub fn is_thinking_block_incompatible(msg: &str) -> bool {
        let lower = msg.to_lowercase();
        lower.contains("redacted_thinking")
            || lower.contains("invalid data in redacted")
            || (lower.contains("thinking") && lower.contains("invalid"))
            || (lower.contains("reasoning") && lower.contains("encrypted_content"))
    }
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

/// Create a provider from its kind and connection parameters.
///
/// This is the single factory for all provider construction — used by both
/// `provider_for_alias` (daemon runtime) and onboarding validation.
///
/// When `api_key` is `Some`, key-based providers (OpenRouter, Kimi) use
/// it directly instead of reading from environment variables. This is
/// needed during onboarding when the key hasn't been persisted yet.
pub fn create_provider(
    kind: ProviderKind,
    headers: std::collections::BTreeMap<String, String>,
    routing: Option<crate::providers::openai_compatible::ProviderRouting>,
    api_key: Option<&str>,
    base_url: Option<&str>,
    api_key_env: Option<&str>,
) -> Result<Arc<dyn Provider>, ProviderInitError> {
    match kind {
        ProviderKind::OpenRouter => Ok(Arc::new(match api_key {
            Some(key) => crate::providers::openrouter::OpenRouterProvider::new_with_key(
                key, headers, routing,
            )?,
            None => crate::providers::openrouter::OpenRouterProvider::new(headers, routing)?,
        })),
        ProviderKind::OpenAiCompatible => {
            let endpoint = base_url.ok_or_else(|| ProviderInitError::UnsupportedProvider {
                provider: "openai_compatible requires base_url".to_string(),
            })?;
            Ok(Arc::new(match api_key {
                Some(key) => crate::providers::openai_compatible_provider::OpenAiCompatibleProvider::with_auth_key(
                    "openai_compatible",
                    endpoint,
                    key,
                    reqwest::header::HeaderMap::new(),
                    headers,
                    None,
                )?,
                None => crate::providers::openai_compatible_provider::OpenAiCompatibleProvider::with_optional_auth_env(
                    "openai_compatible",
                    endpoint,
                    api_key_env,
                    reqwest::header::HeaderMap::new(),
                    headers,
                    None,
                )?,
            }))
        }
        ProviderKind::Kimi => Ok(Arc::new(match api_key {
            Some(key) => crate::providers::kimi_code::KimiCodeProvider::new_with_key(key, headers)?,
            None => crate::providers::kimi_code::KimiCodeProvider::new(headers)?,
        })),
        ProviderKind::OpenAiOAuth => Ok(Arc::new(
            crate::providers::openai_oauth::OpenAiOAuthProvider::new(headers)?,
        )),
        ProviderKind::Anthropic => Ok(Arc::new(
            crate::providers::anthropic::AnthropicProvider::new(headers)?,
        )),
    }
}

#[tracing::instrument(skip_all, fields(alias = alias.unwrap_or(&config.models.default)))]
pub fn provider_for_alias(
    config: &Config,
    alias: Option<&str>,
) -> Result<Arc<dyn Provider>, ProviderInitError> {
    let (_alias, model) = model_from_alias(config, alias)?;
    let debug = (
        config.debug.save_requests,
        config.workspace.as_path(),
        config.debug.max_saved_requests,
    );

    match model.provider {
        ProviderKind::OpenRouter => {
            let mut p = crate::providers::openrouter::OpenRouterProvider::new(
                model.headers.clone(),
                model.provider_routing.clone(),
            )?;
            p.set_debug(debug.0, debug.1, debug.2);
            Ok(Arc::new(p))
        }
        ProviderKind::OpenAiCompatible => {
            let endpoint = model
                .base_url
                .as_deref()
                .ok_or_else(|| ProviderInitError::UnsupportedProvider {
                    provider: format!(
                        "openai_compatible model alias '{}' requires base_url",
                        alias.unwrap_or(&config.models.default)
                    ),
                })?;
            let mut p = crate::providers::openai_compatible_provider::OpenAiCompatibleProvider::with_optional_auth_env(
                "openai_compatible",
                endpoint,
                model.api_key_env.as_deref(),
                reqwest::header::HeaderMap::new(),
                model.headers.clone(),
                None,
            )?;
            p.set_debug(debug.0, debug.1, debug.2);
            Ok(Arc::new(p))
        }
        ProviderKind::Kimi => {
            let mut p = crate::providers::kimi_code::KimiCodeProvider::new(model.headers.clone())?;
            p.set_debug(debug.0, debug.1, debug.2);
            Ok(Arc::new(p))
        }
        ProviderKind::OpenAiOAuth => {
            let mut p =
                crate::providers::openai_oauth::OpenAiOAuthProvider::new(model.headers.clone())?;
            p.set_debug(debug.0, debug.1, debug.2);
            Ok(Arc::new(p))
        }
        ProviderKind::Anthropic => {
            let mut p = crate::providers::anthropic::AnthropicProvider::new(model.headers.clone())?;
            p.set_debug(debug.0, debug.1, debug.2);
            Ok(Arc::new(p))
        }
    }
}

/// Create a provider for a chain of model aliases.
///
/// For single-element chains, returns the inner provider directly
/// (no wrapping). For multi-element chains, returns a `ChainProvider`.
pub fn provider_for_chain(
    config: &Config,
    aliases: &[String],
) -> Result<Arc<dyn Provider>, ProviderInitError> {
    if aliases.is_empty() {
        return Err(ProviderInitError::UnknownAlias {
            alias: "<empty chain>".to_string(),
        });
    }

    if aliases.len() == 1 {
        return provider_for_alias(config, Some(&aliases[0]));
    }

    let mut providers = Vec::with_capacity(aliases.len());
    for alias in aliases {
        let (_, model_config) = model_from_alias(config, Some(alias))?;
        let model_name = model_config.model.clone();
        let provider = provider_for_alias(config, Some(alias))?;
        providers.push((alias.clone(), provider, model_name));
    }

    Ok(Arc::new(crate::providers::chain::ChainProvider::new(
        providers,
    )))
}

/// Create a provider from a `StringOrList` model reference.
///
/// This is the primary entry point for all provider creation at
/// runtime. Dispatches to `provider_for_alias` (single string) or
/// `provider_for_chain` (list).
pub fn provider_for_model_ref(
    config: &Config,
    model_ref: &crate::config::StringOrList,
) -> Result<Arc<dyn Provider>, ProviderInitError> {
    let aliases = model_ref.as_slice();
    if aliases.len() == 1 {
        provider_for_alias(config, Some(&aliases[0]))
    } else {
        provider_for_chain(config, aliases)
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
        assert_eq!(ReasoningEffort::Xhigh.as_str(), "xhigh");
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

    #[test]
    fn is_context_overflow_detects_known_patterns() {
        assert!(ProviderError::is_context_overflow_message(
            "Your input exceeds the context window of this model"
        ));
        assert!(ProviderError::is_context_overflow_message(
            "This model's maximum context length is 128000 tokens"
        ));
        assert!(ProviderError::is_context_overflow_message(
            "prompt_length exceeded maximum of 200000"
        ));
        assert!(ProviderError::is_context_overflow_message(
            "too many tokens"
        ));
        assert!(ProviderError::is_context_overflow_message(
            "prompt is too long"
        ));
        assert!(ProviderError::is_context_overflow_message(
            "input tokens exceed the configured limit"
        ));
        assert!(ProviderError::is_context_overflow_message(
            "context_length_exceeded"
        ));
        assert!(ProviderError::is_context_overflow_message(
            "context length exceeded"
        ));
        assert!(ProviderError::is_context_overflow_message(
            "token limit exceeded"
        ));
        assert!(ProviderError::is_context_overflow_message(
            "input is too long"
        ));
        // Anthropic: exceed context limit
        assert!(ProviderError::is_context_overflow_message(
            "input length and `max_tokens` exceed context limit: 188240 + 21333 > 200000, decrease input length or `max_tokens` and try again"
        ));
        // Anthropic 429: extra usage required
        assert!(ProviderError::is_context_overflow_message(
            "Extra usage is required for long context requests."
        ));
        // Gemini: exceeds maximum number of tokens
        assert!(ProviderError::is_context_overflow_message(
            "The input token count (1632254) exceeds the maximum number of tokens allowed (1048576)."
        ));
        // Gemini Vertex AI: input token count is N
        assert!(ProviderError::is_context_overflow_message(
            "Unable to submit request because the input token count is 1251952 but model only supports up to 1048576."
        ));
        // Kimi: token length too long
        assert!(ProviderError::is_context_overflow_message(
            "Input token length too long"
        ));
        // Kimi: exceeded model token limit
        assert!(ProviderError::is_context_overflow_message(
            "Your request exceeded model token limit : 131072"
        ));
        assert!(!ProviderError::is_context_overflow_message("rate limited"));
        assert!(!ProviderError::is_context_overflow_message(
            "invalid api key"
        ));
    }
}
