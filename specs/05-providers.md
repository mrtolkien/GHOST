# 05 — Provider Trait + OpenRouter Adapter

## Overview

The provider system defines a trait for LLM backends and implements OpenRouter as the
sole PoC provider. The trait exists for future extensibility (Anthropic, Gemini, etc.)
but only OpenRouter ships initially.

## Provider Trait

```rust
#[async_trait]
pub trait Provider: Send + Sync {
    /// Send a chat completion request.
    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, ProviderError>;

    /// Provider name for logging/observability.
    fn name(&self) -> &str;
}
```

## Core Types

```rust
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub tools: Option<Vec<ToolDefinition>>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub system: Option<String>,
}

pub struct ChatMessage {
    pub role: Role,
    pub content: Vec<ContentBlock>,
}

pub enum Role {
    User,
    Assistant,
    System,
}

pub enum ContentBlock {
    Text(String),
    ToolUse { id: String, name: String, input: serde_json::Value },
    ToolResult { tool_use_id: String, content: String, is_error: bool },
}

pub struct ChatResponse {
    pub content: Vec<ContentBlock>,
    pub usage: Usage,
    pub stop_reason: StopReason,
    pub model: String,
}

pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cache_read_tokens: Option<u32>,
    pub cache_creation_tokens: Option<u32>,
}

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

    #[error("request failed: {0}")]
    Request(#[from] reqwest::Error),

    #[error("invalid response: {0}")]
    InvalidResponse(String),
}
```

## OpenRouter Implementation

OpenRouter uses the OpenAI-compatible chat completions API with some extensions.

- **Base URL**: `https://openrouter.ai/api/v1/chat/completions`
- **Auth**: `Authorization: Bearer $OPENROUTER_API_KEY`
- **Headers**: `X-Title: ghost` (app identification)

The adapter converts between GHOST's neutral `ChatRequest`/`ChatResponse` types and
OpenRouter's JSON format. OpenRouter supports tool use natively.

### Key details:

- OpenRouter returns upstream model usage in the response
- Rate limits come as HTTP 429 with `Retry-After` header
- Empty responses can happen — implement configurable retry (default: 2 retries)

## Circuit Breaker

Keep the circuit breaker concept from t-koma but simplified for single-provider use:

- Track consecutive failures per model
- After N failures (default 3), open the circuit for 60 seconds
- Log clearly when circuit opens/closes
- This becomes more important when multiple providers are added later

## Model Resolution

Models are configured by alias in config.toml. The `default` key points to an alias:

```toml
[models]
default = "primary"

[models.primary]
provider = "openrouter"
model = "anthropic/claude-sonnet-4-5-20250929"
```

For now, `default` is always a single alias (no chains). Model chains (fallback lists)
are deferred to post-PoC — the trait and config structure support it but the
orchestration logic is not needed yet.

## Other PoC Providers

The provider trait and OpenAI-compatible client built here are the foundation for two
additional PoC providers in separate specs:

- **Kimi Code** ([05a-kimi-code.md](05a-kimi-code.md)) — OpenAI-compatible with custom
  User-Agent header. Wraps the same client.
- **OpenAI OAuth** ([05b-openai-oauth.md](05b-openai-oauth.md)) — OpenAI-compatible with
  OAuth token management instead of static API keys. Wraps the same client with a token
  refresh layer.

## Observability

Every provider call MUST produce a span:

```rust
#[tracing::instrument(skip_all, fields(
    provider = %self.name(),
    model = %request.model,
))]
async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, ProviderError> {
    let start = Instant::now();
    let response = self.send_request(&request).await?;
    let duration = start.elapsed();

    logfire::info!("provider response",
        model = %response.model,
        input_tokens = response.usage.input_tokens,
        output_tokens = response.usage.output_tokens,
        duration_ms = duration.as_millis(),
        stop_reason = ?response.stop_reason,
    );

    Ok(response)
}
```

## Acceptance Criteria

- Provider trait is defined with `chat()` method
- OpenRouter adapter sends requests and parses responses correctly
- Tool use (function calling) works through OpenRouter
- Rate limit errors are detected and surfaced as `ProviderError::RateLimited`
- Empty responses are retried (configurable count)
- Circuit breaker tracks failures and prevents hammering
- All provider calls produce tracing spans with model, tokens, and duration
- Usage is logged to SurrealDB `usage_log` table

## Prior Art

Old code in `../t-koma`:

- `t-koma-gateway/src/providers/provider.rs` — Provider trait definition. Directly
  reusable shape.
- `t-koma-gateway/src/providers/openrouter/` — OpenRouter adapter with request/response
  mapping. Directly reusable, just adapt types.
- `t-koma-gateway/src/circuit_breaker.rs` — Circuit breaker implementation. Reusable
  as-is.
- `t-koma-gateway/src/state.rs` — Model chain resolution and provider instantiation.
  Only relevant later when adding fallback chains.
