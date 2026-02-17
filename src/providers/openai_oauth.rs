use std::collections::BTreeMap;
use std::time::Instant;

use async_trait::async_trait;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue};
use serde::Serialize;
use serde_json::Value;

use crate::auth::openai_oauth::{OpenAiOAuthClient, TokenStore};
use crate::providers::circuit_breaker::CircuitBreaker;
use crate::providers::types::{
    ChatRequest, ChatResponse, ContentBlock, Provider, ProviderError, ResponseFormat, StopReason,
    Usage,
};

const OPENAI_CODEX_RESPONSES_URL: &str = "https://chatgpt.com/backend-api/codex/responses";
const JWT_AUTH_CLAIM_PATH: &str = "https://api.openai.com/auth";
const DEFAULT_EMPTY_RESPONSE_RETRIES: u8 = 2;

#[derive(Debug)]
pub struct OpenAiOAuthProvider {
    client: reqwest::Client,
    oauth_client: OpenAiOAuthClient,
    token_store: TokenStore,
    circuit_breaker: CircuitBreaker,
    empty_response_retries: u8,
    endpoint: String,
    static_headers: HeaderMap,
}

#[derive(Debug, Serialize)]
struct CodexResponsesRequest {
    model: String,
    store: bool,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    instructions: Option<String>,
    input: Vec<CodexInputMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<CodexTextOptions>,
}

#[derive(Debug, Serialize)]
struct CodexInputMessage {
    role: String,
    content: Vec<CodexInputPart>,
}

#[derive(Debug, Serialize)]
struct CodexInputPart {
    r#type: String,
    text: String,
}

#[derive(Debug, Serialize)]
struct CodexTextOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    format: Option<CodexTextFormat>,
}

#[derive(Debug, Serialize)]
struct CodexTextFormat {
    r#type: String,
    name: String,
    schema: Value,
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
            empty_response_retries: DEFAULT_EMPTY_RESPONSE_RETRIES,
            endpoint: OPENAI_CODEX_RESPONSES_URL.to_string(),
            static_headers,
        })
    }

    #[tracing::instrument(skip_all, fields(provider = "openai_oauth", model = %request.model))]
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

        let body = build_codex_request_body(request)?;
        let request_json =
            serde_json::to_string(&body).unwrap_or_else(|e| format!("<serialization failed: {e}>"));
        let started = Instant::now();
        logfire::info!(
            "provider request",
            provider = "openai_oauth",
            model = request.model.clone(),
            endpoint = self.endpoint.clone(),
            messages = body.input.len() as u64,
            body_len = request_json.len() as u64,
        );
        logfire::debug!("provider request body", body = request_json);
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
        let response_body = http_response.text().await?;

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
        if !status.is_success() {
            self.circuit_breaker.record_failure(&request.model);
            return Err(ProviderError::InvalidResponse(format!(
                "http status {status}: {}",
                extract_error_message(&response_body)
            )));
        }

        let parsed =
            parse_codex_sse_response(&response_body, &request.model).inspect_err(|error| {
                logfire::error!(
                    "oauth provider response parse failed",
                    provider = "openai_oauth",
                    model = request.model.clone(),
                    error = error.to_string(),
                    raw_response = response_body.clone()
                );
            })?;
        self.circuit_breaker.record_success(&request.model);

        let tool_call_summary: String = parsed
            .content
            .iter()
            .filter_map(|block| match block {
                ContentBlock::ToolUse { name, .. } => Some(name.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(", ");
        let content_json = serde_json::to_string(&parsed.content)
            .unwrap_or_else(|e| format!("<serialization failed: {e}>"));

        logfire::info!(
            "provider response",
            provider = "openai_oauth",
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
impl Provider for OpenAiOAuthProvider {
    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, ProviderError> {
        let max_attempts = usize::from(self.empty_response_retries) + 1;
        for attempt in 0..max_attempts {
            match self.send_request(&request).await {
                Ok(response) => return Ok(response),
                Err(ProviderError::EmptyResponse) if attempt + 1 < max_attempts => {
                    logfire::warn!(
                        "provider returned empty response; retrying",
                        provider = "openai_oauth",
                        model = request.model.clone(),
                        attempt = attempt + 1
                    );
                }
                Err(error) => return Err(error),
            }
        }
        Err(ProviderError::EmptyResponse)
    }

    fn name(&self) -> &str {
        "openai_oauth"
    }
}

fn build_codex_request_body(request: &ChatRequest) -> Result<CodexResponsesRequest, ProviderError> {
    let mut input = Vec::new();
    for message in &request.messages {
        let (role, part_type) = match message.role {
            crate::providers::Role::User => ("user", "input_text"),
            crate::providers::Role::Assistant => ("assistant", "output_text"),
            crate::providers::Role::System => ("developer", "input_text"),
        };
        let text = message
            .content
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        if text.trim().is_empty() {
            continue;
        }
        input.push(CodexInputMessage {
            role: role.to_string(),
            content: vec![CodexInputPart {
                r#type: part_type.to_string(),
                text,
            }],
        });
    }

    if input.is_empty() {
        return Err(ProviderError::InvalidResponse(
            "request must include at least one text message".to_string(),
        ));
    }

    let text = request.response_format.as_ref().map(|format| match format {
        ResponseFormat::JsonSchema { name, schema } => CodexTextOptions {
            format: Some(CodexTextFormat {
                r#type: "json_schema".to_string(),
                name: name.clone(),
                schema: schema.clone(),
            }),
        },
    });

    Ok(CodexResponsesRequest {
        model: request.model.clone(),
        store: false,
        stream: true,
        instructions: request.system.clone(),
        input,
        text,
    })
}

fn parse_codex_sse_response(
    raw: &str,
    fallback_model: &str,
) -> Result<ChatResponse, ProviderError> {
    let mut completed_response: Option<Value> = None;
    let mut output_text = String::new();

    for chunk in raw.split("\n\n") {
        for line in chunk.lines() {
            let line = line.trim();
            if !line.starts_with("data:") {
                continue;
            }
            let data = line.trim_start_matches("data:").trim();
            if data.is_empty() || data == "[DONE]" {
                continue;
            }
            let event_value: Value = match serde_json::from_str(data) {
                Ok(value) => value,
                Err(_) => continue,
            };
            let event_type = event_value
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("");
            match event_type {
                "response.output_text.delta" => {
                    if let Some(delta) = event_value.get("delta").and_then(Value::as_str) {
                        output_text.push_str(delta);
                    }
                }
                "response.completed" | "response.done" => {
                    if let Some(response) = event_value.get("response") {
                        completed_response = Some(response.clone());
                    }
                }
                _ => {}
            }
        }
    }

    if let Some(value) = completed_response {
        return parse_codex_response_value(&value, fallback_model);
    }
    if !output_text.trim().is_empty() {
        return Ok(ChatResponse {
            content: vec![ContentBlock::Text { text: output_text }],
            usage: Usage::default(),
            stop_reason: StopReason::EndTurn,
            model: fallback_model.to_string(),
        });
    }
    Err(ProviderError::EmptyResponse)
}

fn parse_codex_response_value(
    value: &Value,
    fallback_model: &str,
) -> Result<ChatResponse, ProviderError> {
    let model = value
        .get("model")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .unwrap_or_else(|| fallback_model.to_string());
    let stop_reason = match value.get("status").and_then(Value::as_str) {
        Some("incomplete") => StopReason::MaxTokens,
        Some("completed") => StopReason::EndTurn,
        _ => StopReason::EndTurn,
    };

    let mut content = Vec::new();
    if let Some(output) = value.get("output").and_then(Value::as_array) {
        for item in output {
            if item.get("type").and_then(Value::as_str) != Some("message") {
                continue;
            }
            if let Some(parts) = item.get("content").and_then(Value::as_array) {
                for part in parts {
                    if let Some(text) = part
                        .get("text")
                        .and_then(Value::as_str)
                        .or_else(|| part.get("output_text").and_then(Value::as_str))
                        && !text.trim().is_empty()
                    {
                        content.push(ContentBlock::Text {
                            text: text.to_string(),
                        });
                    }
                }
            }
        }
    }
    if content.is_empty()
        && let Some(text) = value
            .get("output_text")
            .and_then(Value::as_str)
            .filter(|text| !text.trim().is_empty())
    {
        content.push(ContentBlock::Text {
            text: text.to_string(),
        });
    }

    if content.is_empty() {
        return Err(ProviderError::EmptyResponse);
    }

    let usage_value = value.get("usage");
    let input_tokens = usage_value
        .and_then(|usage| {
            usage
                .get("input_tokens")
                .or_else(|| usage.get("prompt_tokens"))
        })
        .and_then(Value::as_u64)
        .unwrap_or(0) as u32;
    let output_tokens = usage_value
        .and_then(|usage| {
            usage
                .get("output_tokens")
                .or_else(|| usage.get("completion_tokens"))
        })
        .and_then(Value::as_u64)
        .unwrap_or(0) as u32;
    let cache_read_tokens = usage_value
        .and_then(|usage| {
            usage
                .get("input_tokens_details")
                .and_then(|details| details.get("cached_tokens"))
                .or_else(|| usage.get("cache_read_input_tokens"))
        })
        .and_then(Value::as_u64)
        .map(|value| value as u32);

    Ok(ChatResponse {
        content,
        usage: Usage {
            input_tokens,
            output_tokens,
            cache_read_tokens,
            cache_creation_tokens: None,
        },
        stop_reason,
        model,
    })
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
    use super::*;
    use crate::providers::{ChatMessage, Role};

    #[test]
    fn parses_account_id_from_jwt_claim() {
        let payload = serde_json::json!({
            "https://api.openai.com/auth": { "chatgpt_account_id": "acc_123" }
        });
        let encoded = URL_SAFE_NO_PAD.encode(payload.to_string().as_bytes());
        let token = format!("header.{encoded}.sig");
        assert_eq!(extract_account_id(&token).as_deref(), Some("acc_123"));
    }

    #[test]
    fn build_request_uses_output_text_for_assistant_messages() {
        let request = ChatRequest {
            messages: vec![
                ChatMessage {
                    role: Role::User,
                    content: vec![ContentBlock::Text {
                        text: "user text".to_string(),
                    }],
                },
                ChatMessage {
                    role: Role::Assistant,
                    content: vec![ContentBlock::Text {
                        text: "assistant text".to_string(),
                    }],
                },
            ],
            model: "gpt-5.3-codex".to_string(),
            max_tokens: None,
            temperature: None,
            system: None,
            response_format: None,
            tools: None,
        };

        let body = build_codex_request_body(&request).expect("request body");

        assert_eq!(body.input[0].role, "user");
        assert_eq!(body.input[0].content[0].r#type, "input_text");
        assert_eq!(body.input[1].role, "assistant");
        assert_eq!(body.input[1].content[0].r#type, "output_text");
    }
}
