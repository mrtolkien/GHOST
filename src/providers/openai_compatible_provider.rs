use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

use async_trait::async_trait;
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderName, HeaderValue};
use tracing::Span;

use crate::providers::circuit_breaker::CircuitBreaker;
use crate::providers::openai_compatible::{
    ChatCompletionsResponse, ProviderRouting, build_request_body, parse_response,
};
use crate::providers::types::{ChatRequest, ChatResponse, Provider, ProviderError};

#[derive(Debug)]
pub struct OpenAiCompatibleProvider {
    client: reqwest::Client,
    circuit_breaker: CircuitBreaker,
    endpoint: String,
    provider_name: &'static str,
    provider_routing: Option<ProviderRouting>,
    debug_save_requests: bool,
    debug_dir: Option<PathBuf>,
}

impl OpenAiCompatibleProvider {
    #[tracing::instrument(name = "create provider", skip_all, fields(provider = provider_name, endpoint = endpoint))]
    pub fn with_auth_env(
        provider_name: &'static str,
        endpoint: &'static str,
        auth_env_var: &'static str,
        mut default_headers: HeaderMap,
        extra_headers: BTreeMap<String, String>,
        provider_routing: Option<ProviderRouting>,
    ) -> Result<Self, ProviderError> {
        let api_key = std::env::var(auth_env_var)
            .map_err(|_| ProviderError::Auth(format!("{auth_env_var} is not set")))?;

        default_headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {api_key}")).map_err(|error| {
                ProviderError::InvalidResponse(format!(
                    "failed to encode authorization header: {error}"
                ))
            })?,
        );

        for (name, value) in extra_headers {
            if let (Ok(header_name), Ok(header_value)) = (
                HeaderName::try_from(name.as_str()),
                HeaderValue::from_str(&value),
            ) {
                default_headers.insert(header_name, header_value);
            }
        }

        let client = reqwest::Client::builder()
            .default_headers(default_headers)
            .build()
            .map_err(ProviderError::Request)?;

        Ok(Self {
            client,
            circuit_breaker: CircuitBreaker::default(),
            endpoint: endpoint.to_string(),
            provider_name,
            provider_routing,
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

    #[cfg(test)]
    pub(crate) fn new_for_tests(provider_name: &'static str, endpoint: impl Into<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            circuit_breaker: CircuitBreaker::new(2, std::time::Duration::from_secs(10)),
            endpoint: endpoint.into(),
            provider_name,
            provider_routing: None,
            debug_save_requests: false,
            debug_dir: None,
        }
    }

    #[tracing::instrument(
        name = "request completion",
        skip_all,
        fields(
            gen_ai.system = self.provider_name,
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
    /// Send a chat request to the OpenAI-compatible endpoint, handling circuit
    /// breaking, debug logging, error classification, and OTel span recording.
    async fn send_request(&self, request: &ChatRequest) -> Result<ChatResponse, ProviderError> {
        if let Some(retry_after_secs) = self.circuit_breaker.check(&request.model) {
            return Err(ProviderError::CircuitOpen {
                model: request.model.clone(),
                retry_after_secs,
            });
        }

        let body = build_request_body(request, self.provider_routing.as_ref());
        let request_json =
            serde_json::to_string(&body).unwrap_or_else(|e| format!("<serialization failed: {e}>"));
        let started = Instant::now();
        tracing::info!(body = request_json.clone(), "provider request body");
        let http_response = self.client.post(&self.endpoint).json(&body).send().await?;
        let status = http_response.status();
        let retry_after_secs = parse_retry_after_secs(
            http_response
                .headers()
                .get("Retry-After")
                .and_then(|value| value.to_str().ok()),
        );
        let response_body = http_response.text().await?;
        let duration_ms = started.elapsed().as_millis() as u64;

        if self.debug_save_requests
            && let Some(ref dir) = self.debug_dir
        {
            crate::providers::debug::save_debug_request(
                &crate::providers::debug::DebugRequestData {
                    dir,
                    provider_name: self.provider_name,
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
            return Err(ProviderError::RateLimited { retry_after_secs });
        }

        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            self.circuit_breaker.record_failure(&request.model);
            return Err(ProviderError::Auth(format!(
                "HTTP {status}: {response_body}"
            )));
        }

        if status == reqwest::StatusCode::NOT_FOUND {
            self.circuit_breaker.record_failure(&request.model);
            return Err(ProviderError::ModelNotFound(format!(
                "{}: HTTP {status}: {response_body}",
                request.model
            )));
        }

        if status.is_server_error() {
            self.circuit_breaker.record_failure(&request.model);
            return Err(ProviderError::ServerError {
                status: status.as_u16(),
                message: response_body,
            });
        }
        if !status.is_success() {
            let err_msg = format!("HTTP {status}: {response_body}");
            if ProviderError::is_context_overflow_message(&response_body) {
                return Err(ProviderError::ContextOverflow(err_msg));
            }
            if ProviderError::is_thinking_block_incompatible(&response_body) {
                return Err(ProviderError::IncompatibleHistory(err_msg));
            }
            return Err(ProviderError::InvalidResponse(err_msg));
        }

        let response: ChatCompletionsResponse =
            serde_json::from_str(&response_body).map_err(|error| {
                tracing::error!(
                    provider = self.provider_name,
                    model = request.model.clone(),
                    error = error.to_string(),
                    raw_response = response_body.clone(),
                    "provider response was not valid json",
                );
                ProviderError::InvalidResponse(format!("response body is not valid json: {error}"))
            })?;
        let mut parsed = match parse_response(response) {
            Ok(parsed) => parsed,
            Err(ProviderError::EmptyResponse { detail }) => {
                tracing::warn!(
                    provider = self.provider_name,
                    model = request.model.clone(),
                    status = status.as_u16() as u64,
                    detail = detail.clone(),
                    raw_response = response_body.clone(),
                    "provider response parsed as empty",
                );
                return Err(ProviderError::EmptyResponse { detail });
            }
            Err(error) => {
                tracing::error!(
                    provider = self.provider_name,
                    model = request.model.clone(),
                    error = error.to_string(),
                    raw_response = response_body.clone(),
                    "provider response parse failed",
                );
                return Err(error);
            }
        };
        if parsed.model.is_empty() {
            parsed.model = request.model.clone();
        }

        self.circuit_breaker.record_success(&request.model);

        let response_json = serde_json::to_string(&parsed.content)
            .unwrap_or_else(|e| format!("<serialization failed: {e}>"));
        tracing::info!(content = response_json, "provider response content");

        let tool_call_summary: String = parsed
            .content
            .iter()
            .filter_map(|block| match block {
                crate::providers::ContentBlock::ToolUse { name, .. } => Some(name.as_str()),
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
impl Provider for OpenAiCompatibleProvider {
    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, ProviderError> {
        self.send_request(&request).await
    }

    fn name(&self) -> &str {
        self.provider_name
    }
}

fn parse_retry_after_secs(retry_after: Option<&str>) -> Option<u64> {
    retry_after.and_then(|value| value.trim().parse::<u64>().ok())
}
