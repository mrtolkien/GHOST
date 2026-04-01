/// Anthropic Messages API provider using Claude Code OAuth credentials.
///
/// Talks to `api.anthropic.com/v1/messages` with SSE streaming, prompt
/// caching, thinking support, and Claude Code tool name translation.
mod credentials;
mod messages;
mod streaming;
mod tool_names;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::RwLock;
use std::time::Instant;

use async_trait::async_trait;
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue};
use tracing::Span;

use self::credentials::OAuthCredentials;
use crate::providers::circuit_breaker::CircuitBreaker;
use crate::providers::types::{ChatRequest, ChatResponse, ContentBlock, Provider, ProviderError};

const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1/messages?beta=true";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const USER_AGENT: &str = "claude-cli/2.1.86 (external, sdk-cli)";
const BASE_BETA_FLAGS: &str = "claude-code-20250219,oauth-2025-04-20,interleaved-thinking-2025-05-14,prompt-caching-scope-2026-01-05,advanced-tool-use-2025-11-20,effort-2025-11-24";

#[derive(Debug)]
pub struct AnthropicProvider {
    client: reqwest::Client,
    credentials: RwLock<OAuthCredentials>,
    /// `None` when credentials came from an env var (no refresh possible).
    credentials_path: Option<PathBuf>,
    circuit_breaker: CircuitBreaker,
    static_headers: HeaderMap,
    debug_save_requests: bool,
    debug_dir: Option<PathBuf>,
    debug_max_saved: usize,
}

impl AnthropicProvider {
    #[tracing::instrument(skip_all)]
    pub fn new(extra_headers: BTreeMap<String, String>) -> Result<Self, ProviderError> {
        let (creds, creds_path) = credentials::load_credentials()?;

        let client = reqwest::Client::builder()
            .build()
            .map_err(ProviderError::Request)?;

        let mut static_headers = HeaderMap::new();
        static_headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        static_headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        static_headers.insert(
            "anthropic-version",
            HeaderValue::from_static(ANTHROPIC_VERSION),
        );
        static_headers.insert(
            "anthropic-dangerous-direct-browser-access",
            HeaderValue::from_static("true"),
        );
        static_headers.insert("user-agent", HeaderValue::from_static(USER_AGENT));
        static_headers.insert("x-app", HeaderValue::from_static("cli"));

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
            credentials: RwLock::new(creds),
            credentials_path: creds_path,
            circuit_breaker: CircuitBreaker::default(),
            static_headers,
            debug_save_requests: false,
            debug_dir: None,
            debug_max_saved: 0,
        })
    }

    pub fn set_debug(&mut self, save: bool, workspace: &std::path::Path, max_saved: usize) {
        self.debug_save_requests = save;
        if save {
            self.debug_dir = Some(workspace.join("debug/requests"));
        }
        self.debug_max_saved = max_saved;
    }

    #[tracing::instrument(
        name = "request completion",
        skip_all,
        fields(
            gen_ai.system = "anthropic",
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
    async fn send_request(&self, request: &ChatRequest) -> Result<ChatResponse, ProviderError> {
        // --- Circuit breaker ---
        if let Some(retry_after_secs) = self.circuit_breaker.check(&request.model) {
            return Err(ProviderError::CircuitOpen {
                model: request.model.clone(),
                retry_after_secs,
            });
        }

        // --- Credential refresh ---
        let access_token = self.ensure_valid_token().await?;

        // --- Headers ---
        let mut headers = self.static_headers.clone();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {access_token}")).map_err(|e| {
                ProviderError::InvalidResponse(format!(
                    "failed to encode authorization header: {e}"
                ))
            })?,
        );

        let beta_header = BASE_BETA_FLAGS;
        headers.insert(
            "anthropic-beta",
            HeaderValue::from_str(beta_header).map_err(|e| {
                ProviderError::InvalidResponse(format!("failed to encode beta header: {e}"))
            })?,
        );

        // --- Build request body ---
        let ghost_tool_names: Vec<&str> = request
            .tools
            .as_ref()
            .map(|tools| tools.iter().map(|t| t.name.as_str()).collect())
            .unwrap_or_default();

        let body = messages::build_request_body(request, &ghost_tool_names)?;
        let request_json =
            serde_json::to_string(&body).unwrap_or_else(|e| format!("<serialization failed: {e}>"));

        tracing::info!(body = request_json.clone(), "provider request body");

        // --- HTTP request ---
        let started = Instant::now();

        let http_response = self
            .client
            .post(ANTHROPIC_API_URL)
            .headers(headers)
            .json(&body)
            .send()
            .await?;

        let status = http_response.status();
        let retry_after_secs = parse_retry_after_secs(
            http_response
                .headers()
                .get("Retry-After")
                .and_then(|v| v.to_str().ok()),
        );
        let response_body = http_response.text().await?;
        let duration_ms = started.elapsed().as_millis() as u64;

        // --- Debug save ---
        if self.debug_save_requests
            && let Some(ref dir) = self.debug_dir
        {
            crate::providers::debug::save_debug_request(
                &crate::providers::debug::DebugRequestData {
                    dir,
                    provider_name: "anthropic",
                    model: &request.model,
                    request_body: &request_json,
                    response_body: &response_body,
                    status: status.as_u16(),
                    duration_ms,
                    debug_context: request.debug_context.as_ref(),
                    max_saved_requests: self.debug_max_saved,
                },
            );
        }

        // --- Error handling ---
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            self.circuit_breaker.record_failure(&request.model);
            // Anthropic returns 429 for long-context billing errors
            // (e.g. "Extra usage is required for long context requests").
            // Classify these as ContextOverflow so the retry path compacts
            // history instead of waiting for a rate-limit reset.
            if ProviderError::is_context_overflow_message(&response_body) {
                return Err(ProviderError::ContextOverflow(format!(
                    "HTTP {status}: {response_body}"
                )));
            }
            tracing::warn!(
                provider = "anthropic",
                model = request.model.clone(),
                retry_after_secs = retry_after_secs,
                raw_response = response_body.clone(),
                "anthropic provider rate limited",
            );
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
            tracing::warn!(
                provider = "anthropic",
                status = status.as_u16(),
                raw_response = response_body.clone(),
                "anthropic provider non-success response",
            );
            let err_msg = format!("HTTP {status}: {response_body}");
            if ProviderError::is_context_overflow_message(&response_body) {
                return Err(ProviderError::ContextOverflow(err_msg));
            }
            if ProviderError::is_thinking_block_incompatible(&response_body) {
                return Err(ProviderError::IncompatibleHistory(err_msg));
            }
            return Err(ProviderError::InvalidResponse(err_msg));
        }

        // --- Parse SSE response ---
        let parsed =
            streaming::parse_sse_response(&response_body, &request.model, &ghost_tool_names)
                .inspect_err(|error| {
                    tracing::error!(
                        provider = "anthropic",
                        model = request.model.clone(),
                        error = error.to_string(),
                        raw_response = response_body.clone(),
                        "anthropic provider response parse failed",
                    );
                })?;

        self.circuit_breaker.record_success(&request.model);

        // --- OTel span recording ---
        let response_json = serde_json::to_string(&parsed.content)
            .unwrap_or_else(|e| format!("<serialization failed: {e}>"));
        tracing::info!(content = response_json, "provider response content");

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

    /// Return a valid access token, refreshing if expired and file-based.
    ///
    /// When the in-memory token is expired, re-reads from disk first — Claude
    /// Code may have already refreshed and written valid tokens. Only falls
    /// back to a network refresh if the disk credentials are also expired.
    async fn ensure_valid_token(&self) -> Result<String, ProviderError> {
        // Fast path: read lock to check if token is still valid.
        {
            let creds = self.credentials.read().expect("credentials lock poisoned");
            if !creds.is_expired() {
                return Ok(creds.access_token.clone());
            }
        }

        // Token is expired. If env-var based, we can't refresh.
        let Some(ref creds_path) = self.credentials_path else {
            let creds = self.credentials.read().expect("credentials lock poisoned");
            return Ok(creds.access_token.clone());
        };

        // Re-read from disk — Claude Code may have refreshed since we last
        // loaded. Use the disk credentials if they're still valid.
        let disk_creds = credentials::read_credentials_from_path(creds_path);
        if let Ok(ref fresh) = disk_creds
            && !fresh.is_expired()
        {
            let token = fresh.access_token.clone();
            let mut creds = self.credentials.write().expect("credentials lock poisoned");
            *creds = fresh.clone();
            tracing::debug!("picked up refreshed credentials from disk");
            return Ok(token);
        }

        // Disk credentials are also expired (or unreadable). Refresh using
        // the disk refresh token (not the stale in-memory one).
        let creds_for_refresh = disk_creds.unwrap_or_else(|_| {
            self.credentials
                .read()
                .expect("credentials lock poisoned")
                .clone()
        });
        let path = creds_path.clone();

        // Perform refresh (async, no lock held).
        let new_creds = credentials::refresh_token(&self.client, &creds_for_refresh, &path).await?;

        let token = new_creds.access_token.clone();

        // Write lock to update stored credentials.
        {
            let mut creds = self.credentials.write().expect("credentials lock poisoned");
            *creds = new_creds;
        }

        Ok(token)
    }
}

#[async_trait]
impl Provider for AnthropicProvider {
    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, ProviderError> {
        self.send_request(&request).await
    }

    fn name(&self) -> &str {
        "anthropic"
    }
}

fn parse_retry_after_secs(retry_after: Option<&str>) -> Option<u64> {
    retry_after.and_then(|v| v.trim().parse::<u64>().ok())
}
