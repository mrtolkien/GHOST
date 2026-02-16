use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::{Config, ModelConfig};

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<ResponseFormat>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResponseFormat {
    JsonSchema { name: String, schema: Value },
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
    Text(String),
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

    #[error("empty response from provider")]
    EmptyResponse,

    #[error("circuit breaker open for model '{model}' (retry after {retry_after_secs}s)")]
    CircuitOpen {
        model: String,
        retry_after_secs: u64,
    },

    #[error("request failed: {0}")]
    Request(#[from] reqwest::Error),

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
        "openrouter" => Ok(Arc::new(
            crate::providers::openrouter::OpenRouterProvider::new(model.headers.clone())?,
        )),
        unsupported => Err(ProviderInitError::UnsupportedProvider {
            provider: unsupported.to_string(),
        }),
    }
}

#[must_use]
pub fn user_message(content: impl Into<String>) -> ChatMessage {
    ChatMessage {
        role: Role::User,
        content: vec![ContentBlock::Text(content.into())],
    }
}
