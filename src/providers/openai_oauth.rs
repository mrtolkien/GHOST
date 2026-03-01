use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

use async_trait::async_trait;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue};
use serde_json::Value;
use tracing::Span;

use crate::auth::openai_oauth::{OpenAiOAuthClient, TokenStore};
use crate::providers::circuit_breaker::CircuitBreaker;
use crate::providers::codex_responses::{build_codex_request_body, parse_codex_response};
use crate::providers::types::{ChatRequest, ChatResponse, ContentBlock, Provider, ProviderError};

const OPENAI_CODEX_RESPONSES_URL: &str = "https://chatgpt.com/backend-api/codex/responses";
const JWT_AUTH_CLAIM_PATH: &str = "https://api.openai.com/auth";

#[derive(Debug)]
pub struct OpenAiOAuthProvider {
    client: reqwest::Client,
    oauth_client: OpenAiOAuthClient,
    token_store: TokenStore,
    circuit_breaker: CircuitBreaker,
    endpoint: String,
    static_headers: HeaderMap,
    debug_save_requests: bool,
    debug_dir: Option<PathBuf>,
}

impl OpenAiOAuthProvider {
    #[tracing::instrument(skip_all)]
    pub fn new(extra_headers: BTreeMap<String, String>) -> Result<Self, ProviderError> {
        let client = reqwest::Client::builder()
            .build()
            .map_err(ProviderError::Request)?;
        let oauth_client =
            OpenAiOAuthClient::new().map_err(|error| ProviderError::Auth(error.to_string()))?;
        let token_store = TokenStore::default_openai_store()
            .map_err(|error| ProviderError::Auth(error.to_string()))?;
        let mut static_headers = HeaderMap::new();
        static_headers.insert(ACCEPT, HeaderValue::from_static("text/event-stream"));
        static_headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        static_headers.insert(
            "OpenAI-Beta",
            HeaderValue::from_static("responses=experimental"),
        );
        static_headers.insert("originator", HeaderValue::from_static("ghost"));

        for (name, value) in extra_headers {
            if let (Ok(header_name), Ok(header_value)) = (
                HeaderName::try_from(name.as_str()),
                HeaderValue::from_str(&value),
            ) {
                static_headers.insert(header_name, header_value);
            }
        }

        Ok(Self {
            client,
            oauth_client,
            token_store,
            circuit_breaker: CircuitBreaker::default(),
            endpoint: OPENAI_CODEX_RESPONSES_URL.to_string(),
            static_headers,
            debug_save_requests: false,
            debug_dir: None,
        })
    }

    pub fn set_debug(&mut self, save: bool, workspace: &std::path::Path) {
        self.debug_save_requests = save;
        if save {
            self.debug_dir = Some(workspace.join("debug/requests"));
        }
    }

    #[tracing::instrument(
        name = "request - openai oauth",
        skip_all,
        fields(
            gen_ai.system = "openai_oauth",
            gen_ai.operation.name = "chat",
            gen_ai.request.model = %request.model,
            gen_ai.response.model = tracing::field::Empty,
            gen_ai.response.id = tracing::field::Empty,
            gen_ai.response.finish_reasons = tracing::field::Empty,
            gen_ai.usage.input_tokens = tracing::field::Empty,
            gen_ai.usage.output_tokens = tracing::field::Empty,
            gen_ai.usage.cache_read_input_tokens = tracing::field::Empty,
            gen_ai.usage.cache_creation_input_tokens = tracing::field::Empty,
            duration_ms = tracing::field::Empty,
            tool_calls = tracing::field::Empty,
        )
    )]
    /// Send a chat request via the ChatGPT OAuth backend (Codex Responses API).
    ///
    /// Handles OAuth token refresh, JWT account-ID extraction, circuit breaking,
    /// SSE response parsing, and OTel span recording.
    async fn send_request(&self, request: &ChatRequest) -> Result<ChatResponse, ProviderError> {
        if let Some(retry_after_secs) = self.circuit_breaker.check(&request.model) {
            return Err(ProviderError::CircuitOpen {
                model: request.model.clone(),
                retry_after_secs,
            });
        }

        let access_token = self
            .token_store
            .get_valid_access_token(&self.oauth_client)
            .await
            .map_err(|error| ProviderError::Auth(error.to_string()))?;
        let account_id = extract_account_id(&access_token).ok_or_else(|| {
            ProviderError::Auth(
                "failed to extract chatgpt account id from access token".to_string(),
            )
        })?;

        let mut headers = self.static_headers.clone();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {access_token}")).map_err(|error| {
                ProviderError::InvalidResponse(format!(
                    "failed to encode authorization header: {error}"
                ))
            })?,
        );
        headers.insert(
            "chatgpt-account-id",
            HeaderValue::from_str(&account_id)
                .map_err(|error| ProviderError::InvalidResponse(error.to_string()))?,
        );

        // session_id header: stable session routing across turns.
        if !request.cache_key.is_empty()
            && let Ok(v) = HeaderValue::from_str(&request.cache_key)
        {
            headers.insert("session_id", v);
        }

        // x-codex-turn-state: sticky routing within a turn. The server
        // returns this header in its response; we echo it back unchanged
        // in subsequent requests so the load-balancer routes us to the
        // same server that has our warm KV cache.
        if let Some(ref ts) = request.turn_state
            && let Ok(v) = HeaderValue::from_str(ts)
        {
            headers.insert("x-codex-turn-state", v);
        }

        let body = build_codex_request_body(request)?;
        let request_json =
            serde_json::to_string(&body).unwrap_or_else(|e| format!("<serialization failed: {e}>"));

        logfire::info!("provider request body", body = request_json.clone());

        let started = Instant::now();

        let http_response = self
            .client
            .post(&self.endpoint)
            .headers(headers)
            .json(&body)
            .send()
            .await?;
        let status = http_response.status();
        let retry_after_secs = parse_retry_after_secs(
            http_response
                .headers()
                .get("Retry-After")
                .and_then(|value| value.to_str().ok()),
        );
        let turn_state = http_response
            .headers()
            .get("x-codex-turn-state")
            .and_then(|v| v.to_str().ok())
            .map(ToString::to_string);
        let response_body = http_response.text().await?;
        let duration_ms = started.elapsed().as_millis() as u64;

        if self.debug_save_requests
            && let Some(ref dir) = self.debug_dir
        {
            crate::providers::debug::save_debug_request(
                &crate::providers::debug::DebugRequestData {
                    dir,
                    provider_name: "openai_oauth",
                    model: &request.model,
                    request_body: &request_json,
                    response_body: &response_body,
                    status: status.as_u16(),
                    duration_ms,
                    debug_context: request.debug_context.as_ref(),
                },
            );
        }

        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            self.circuit_breaker.record_failure(&request.model);
            logfire::warn!(
                "oauth provider rate limited",
                provider = "openai_oauth",
                model = request.model.clone(),
                retry_after_secs = retry_after_secs,
                raw_response = response_body.clone()
            );
            return Err(ProviderError::RateLimited { retry_after_secs });
        }
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            self.circuit_breaker.record_failure(&request.model);
            return Err(ProviderError::Auth(extract_error_message(&response_body)));
        }
        if status == reqwest::StatusCode::NOT_FOUND {
            self.circuit_breaker.record_failure(&request.model);
            return Err(ProviderError::ModelNotFound(request.model.clone()));
        }
        if status.is_server_error() {
            self.circuit_breaker.record_failure(&request.model);
            return Err(ProviderError::ServerError {
                status: status.as_u16(),
                message: extract_error_message(&response_body),
            });
        }
        if !status.is_success() {
            self.circuit_breaker.record_failure(&request.model);
            return Err(ProviderError::InvalidResponse(format!(
                "http status {status}: {}",
                extract_error_message(&response_body)
            )));
        }

        let mut parsed =
            parse_codex_response(&response_body, &request.model).inspect_err(|error| {
                logfire::error!(
                    "oauth provider response parse failed",
                    provider = "openai_oauth",
                    model = request.model.clone(),
                    error = error.to_string(),
                    raw_response = response_body.clone()
                );
            })?;
        parsed.turn_state = turn_state;
        self.circuit_breaker.record_success(&request.model);

        let response_json = serde_json::to_string(&parsed.content)
            .unwrap_or_else(|e| format!("<serialization failed: {e}>"));
        logfire::info!("provider response content", content = response_json);

        let tool_call_summary: String = parsed
            .content
            .iter()
            .filter_map(|block| match block {
                ContentBlock::ToolUse { name, .. } => Some(name.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(", ");

        let finish_reason = match parsed.stop_reason {
            crate::providers::StopReason::EndTurn => "stop",
            crate::providers::StopReason::ToolUse => "tool_calls",
            crate::providers::StopReason::MaxTokens => "length",
        };

        let span = Span::current();
        span.record("gen_ai.response.model", &parsed.model);
        if let Some(ref id) = parsed.response_id {
            span.record("gen_ai.response.id", id);
        }
        span.record("gen_ai.response.finish_reasons", finish_reason);
        span.record("gen_ai.usage.input_tokens", parsed.usage.input_tokens);
        span.record("gen_ai.usage.output_tokens", parsed.usage.output_tokens);
        span.record(
            "gen_ai.usage.cache_read_input_tokens",
            parsed.usage.cache_read_tokens.unwrap_or(0),
        );
        span.record(
            "gen_ai.usage.cache_creation_input_tokens",
            parsed.usage.cache_creation_tokens.unwrap_or(0),
        );
        span.record("duration_ms", duration_ms);
        span.record("tool_calls", &tool_call_summary);

        Ok(parsed)
    }
}

#[async_trait]
impl Provider for OpenAiOAuthProvider {
    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, ProviderError> {
        self.send_request(&request).await
    }

    fn name(&self) -> &str {
        "openai_oauth"
    }
}

fn parse_retry_after_secs(retry_after: Option<&str>) -> Option<u64> {
    retry_after.and_then(|value| value.trim().parse::<u64>().ok())
}

fn extract_error_message(raw: &str) -> String {
    if let Ok(value) = serde_json::from_str::<Value>(raw) {
        if let Some(message) = value
            .get("error")
            .and_then(|error| {
                error
                    .get("message")
                    .or_else(|| error.get("error_description"))
            })
            .and_then(Value::as_str)
        {
            return message.to_string();
        }
        if let Some(message) = value.get("message").and_then(Value::as_str) {
            return message.to_string();
        }
    }
    raw.to_string()
}

fn extract_account_id(access_token: &str) -> Option<String> {
    let payload = access_token.split('.').nth(1)?;
    let payload_bytes = URL_SAFE_NO_PAD.decode(payload).ok()?;
    let payload_json: Value = serde_json::from_slice(&payload_bytes).ok()?;
    payload_json
        .get(JWT_AUTH_CLAIM_PATH)?
        .get("chatgpt_account_id")?
        .as_str()
        .map(ToString::to_string)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn parses_account_id_from_jwt_claim() {
        let payload = json!({
            "https://api.openai.com/auth": { "chatgpt_account_id": "acc_123" }
        });
        let encoded = URL_SAFE_NO_PAD.encode(payload.to_string().as_bytes());
        let token = format!("header.{encoded}.sig");
        assert_eq!(extract_account_id(&token).as_deref(), Some("acc_123"));
    }
}
