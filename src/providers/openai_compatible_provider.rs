use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

use async_trait::async_trait;
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderName, HeaderValue};

use crate::providers::circuit_breaker::CircuitBreaker;
use crate::providers::openai_compatible::{
    ChatCompletionsResponse, ProviderErrorBody, build_request_body, parse_response,
};
use crate::providers::types::{ChatRequest, ChatResponse, Provider, ProviderError};

#[derive(Debug)]
pub struct OpenAiCompatibleProvider {
    client: reqwest::Client,
    circuit_breaker: CircuitBreaker,
    empty_response_retries: u8,
    endpoint: String,
    provider_name: &'static str,
    debug_save_requests: bool,
    debug_dir: Option<PathBuf>,
}

impl OpenAiCompatibleProvider {
    #[tracing::instrument(skip_all, fields(provider = provider_name, endpoint = endpoint))]
    pub fn with_auth_env(
        provider_name: &'static str,
        endpoint: &'static str,
        auth_env_var: &'static str,
        mut default_headers: HeaderMap,
        extra_headers: BTreeMap<String, String>,
        empty_response_retries: u8,
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
            empty_response_retries,
            endpoint: endpoint.to_string(),
            provider_name,
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
    pub(crate) fn new_for_tests(
        provider_name: &'static str,
        endpoint: impl Into<String>,
        empty_response_retries: u8,
    ) -> Self {
        Self {
            client: reqwest::Client::new(),
            circuit_breaker: CircuitBreaker::new(2, std::time::Duration::from_secs(10)),
            empty_response_retries,
            endpoint: endpoint.into(),
            provider_name,
            debug_save_requests: false,
            debug_dir: None,
        }
    }

    #[tracing::instrument(skip_all, fields(provider = self.provider_name, model = %request.model))]
    async fn send_request(&self, request: &ChatRequest) -> Result<ChatResponse, ProviderError> {
        if let Some(retry_after_secs) = self.circuit_breaker.check(&request.model) {
            return Err(ProviderError::CircuitOpen {
                model: request.model.clone(),
                retry_after_secs,
            });
        }

        let body = build_request_body(request);
        let request_json =
            serde_json::to_string(&body).unwrap_or_else(|e| format!("<serialization failed: {e}>"));
        let started = Instant::now();
        logfire::info!(
            "provider request",
            provider = self.provider_name,
            model = request.model.clone(),
            endpoint = self.endpoint.clone(),
            messages = body.messages.len() as u64,
            tools = body.tools.as_ref().map_or(0, |tools| tools.len()) as u64,
            body_len = request_json.len() as u64,
        );
        logfire::debug!("provider request body", body = request_json.clone(),);
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
            return Err(ProviderError::Auth(extract_error_message(&response_body)));
        }

        if status == reqwest::StatusCode::NOT_FOUND {
            self.circuit_breaker.record_failure(&request.model);
            return Err(ProviderError::ModelNotFound(request.model.clone()));
        }

        if !status.is_success() {
            self.circuit_breaker.record_failure(&request.model);
            return Err(ProviderError::InvalidResponse(format!(
                "http status {status}: {}",
                extract_error_message(&response_body)
            )));
        }

        let response: ChatCompletionsResponse =
            serde_json::from_str(&response_body).map_err(|error| {
                logfire::error!(
                    "provider response was not valid json",
                    provider = self.provider_name,
                    model = request.model.clone(),
                    error = error.to_string(),
                    raw_response = response_body.clone()
                );
                ProviderError::InvalidResponse(format!("response body is not valid json: {error}"))
            })?;
        let mut parsed = match parse_response(response) {
            Ok(parsed) => parsed,
            Err(ProviderError::EmptyResponse { detail }) => {
                logfire::warn!(
                    "provider response parsed as empty",
                    provider = self.provider_name,
                    model = request.model.clone(),
                    status = status.as_u16() as u64,
                    detail = detail.clone(),
                    raw_response = response_body.clone()
                );
                return Err(ProviderError::EmptyResponse { detail });
            }
            Err(error) => {
                logfire::error!(
                    "provider response parse failed",
                    provider = self.provider_name,
                    model = request.model.clone(),
                    error = error.to_string(),
                    raw_response = response_body.clone()
                );
                return Err(error);
            }
        };
        if parsed.model.is_empty() {
            parsed.model = request.model.clone();
        }

        self.circuit_breaker.record_success(&request.model);

        let tool_call_summary: String = parsed
            .content
            .iter()
            .filter_map(|block| match block {
                crate::providers::ContentBlock::ToolUse { name, .. } => Some(name.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(", ");

        let content_json = serde_json::to_string(&parsed.content)
            .unwrap_or_else(|e| format!("<serialization failed: {e}>"));

        logfire::info!(
            "provider response",
            provider = self.provider_name,
            model = parsed.model.clone(),
            input_tokens = parsed.usage.input_tokens,
            output_tokens = parsed.usage.output_tokens,
            duration_ms = started.elapsed().as_millis() as u64,
            stop_reason = format!("{:?}", parsed.stop_reason),
            tool_calls = tool_call_summary,
            content = content_json,
        );

        Ok(parsed)
    }
}

#[async_trait]
impl Provider for OpenAiCompatibleProvider {
    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, ProviderError> {
        let max_attempts = usize::from(self.empty_response_retries) + 1;
        for attempt in 0..max_attempts {
            if attempt > 0 {
                logfire::info!(
                    "provider chat retry",
                    provider = self.provider_name,
                    model = request.model.clone(),
                    attempt = (attempt + 1) as u64,
                    max_attempts = max_attempts as u64
                );
            }
            match self.send_request(&request).await {
                Ok(response) => return Ok(response),
                Err(ProviderError::EmptyResponse { ref detail }) if attempt + 1 < max_attempts => {
                    let delay_secs = 2u64.pow(attempt as u32);
                    logfire::warn!(
                        "provider returned empty response; retrying after {delay_secs}s",
                        provider = self.provider_name,
                        model = request.model.clone(),
                        attempt = attempt + 1,
                        delay_secs = delay_secs,
                        detail = detail.clone(),
                    );
                    tokio::time::sleep(std::time::Duration::from_secs(delay_secs)).await;
                }
                Err(error) => return Err(error),
            }
        }

        logfire::error!(
            "provider exhausted empty-response retries",
            provider = self.provider_name,
            model = request.model.clone(),
            max_attempts = max_attempts as u64
        );
        Err(ProviderError::EmptyResponse {
            detail: "exhausted retries".to_string(),
        })
    }

    fn name(&self) -> &str {
        self.provider_name
    }
}

fn parse_retry_after_secs(retry_after: Option<&str>) -> Option<u64> {
    retry_after.and_then(|value| value.trim().parse::<u64>().ok())
}

fn extract_error_message(body: &str) -> String {
    if body.trim().is_empty() {
        return "empty error response".to_string();
    }

    if let Ok(error_body) = serde_json::from_str::<ProviderErrorBody>(body)
        && let Some(payload) = error_body.error
        && let Some(message) = payload.message
        && !message.trim().is_empty()
    {
        return message;
    }

    body.to_string()
}
