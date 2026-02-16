use std::collections::BTreeMap;
use std::time::Instant;

use async_trait::async_trait;
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderName, HeaderValue};

use crate::providers::circuit_breaker::CircuitBreaker;
use crate::providers::openai_compatible::{
    ChatCompletionsResponse, ProviderErrorBody, build_request_body, parse_response,
};
use crate::providers::types::{ChatRequest, ChatResponse, Provider, ProviderError};

const OPENROUTER_CHAT_COMPLETIONS_URL: &str = "https://openrouter.ai/api/v1/chat/completions";
const OPENROUTER_API_KEY_ENV: &str = "OPENROUTER_API_KEY";
const DEFAULT_EMPTY_RESPONSE_RETRIES: u8 = 2;

#[derive(Debug)]
pub struct OpenRouterProvider {
    client: reqwest::Client,
    circuit_breaker: CircuitBreaker,
    empty_response_retries: u8,
    endpoint: String,
}

impl OpenRouterProvider {
    #[tracing::instrument(skip_all)]
    pub fn new(extra_headers: BTreeMap<String, String>) -> Result<Self, ProviderError> {
        let api_key = std::env::var(OPENROUTER_API_KEY_ENV)
            .map_err(|_| ProviderError::Auth(format!("{OPENROUTER_API_KEY_ENV} is not set")))?;

        let mut headers = HeaderMap::new();
        headers.insert("X-Title", HeaderValue::from_static("ghost"));
        headers.insert(
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
                headers.insert(header_name, header_value);
            }
        }

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .map_err(ProviderError::Request)?;

        Ok(Self {
            client,
            circuit_breaker: CircuitBreaker::default(),
            empty_response_retries: DEFAULT_EMPTY_RESPONSE_RETRIES,
            endpoint: OPENROUTER_CHAT_COMPLETIONS_URL.to_string(),
        })
    }

    #[cfg(test)]
    fn new_for_tests(endpoint: impl Into<String>, empty_response_retries: u8) -> Self {
        Self {
            client: reqwest::Client::new(),
            circuit_breaker: CircuitBreaker::new(2, std::time::Duration::from_secs(10)),
            empty_response_retries,
            endpoint: endpoint.into(),
        }
    }

    #[tracing::instrument(skip_all, fields(model = %request.model, provider = "openrouter"))]
    async fn send_request(&self, request: &ChatRequest) -> Result<ChatResponse, ProviderError> {
        if let Some(retry_after_secs) = self.circuit_breaker.check(&request.model) {
            return Err(ProviderError::CircuitOpen {
                model: request.model.clone(),
                retry_after_secs,
            });
        }

        let body = build_request_body(request);
        let started = Instant::now();
        logfire::info!(
            "provider request",
            provider = "openrouter",
            model = request.model.clone(),
            endpoint = self.endpoint.clone(),
            messages = body.messages.len() as u64,
            tools = body.tools.as_ref().map_or(0, |tools| tools.len()) as u64
        );
        let http_response = self.client.post(&self.endpoint).json(&body).send().await?;
        let status = http_response.status();

        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            self.circuit_breaker.record_failure(&request.model);
            return Err(ProviderError::RateLimited {
                retry_after_secs: parse_retry_after_secs(
                    http_response
                        .headers()
                        .get("Retry-After")
                        .and_then(|value| value.to_str().ok()),
                ),
            });
        }

        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            self.circuit_breaker.record_failure(&request.model);
            return Err(ProviderError::Auth(
                extract_error_message(http_response).await,
            ));
        }

        if status == reqwest::StatusCode::NOT_FOUND {
            self.circuit_breaker.record_failure(&request.model);
            return Err(ProviderError::ModelNotFound(request.model.clone()));
        }

        if !status.is_success() {
            self.circuit_breaker.record_failure(&request.model);
            return Err(ProviderError::InvalidResponse(format!(
                "http status {status}: {}",
                extract_error_message(http_response).await
            )));
        }

        let response_body = http_response.text().await?;
        let response: ChatCompletionsResponse =
            serde_json::from_str(&response_body).map_err(|error| {
                logfire::error!(
                    "provider response was not valid json",
                    provider = "openrouter",
                    model = request.model.clone(),
                    error = error.to_string(),
                    raw_response = response_body.clone()
                );
                ProviderError::InvalidResponse(format!("response body is not valid json: {error}"))
            })?;
        let mut parsed = match parse_response(response) {
            Ok(parsed) => parsed,
            Err(ProviderError::EmptyResponse) => {
                logfire::warn!(
                    "provider response parsed as empty",
                    provider = "openrouter",
                    model = request.model.clone(),
                    status = status.as_u16() as u64,
                    raw_response = response_body.clone()
                );
                return Err(ProviderError::EmptyResponse);
            }
            Err(error) => {
                logfire::error!(
                    "provider response parse failed",
                    provider = "openrouter",
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

        logfire::info!(
            "provider response",
            provider = "openrouter",
            model = parsed.model.clone(),
            input_tokens = parsed.usage.input_tokens,
            output_tokens = parsed.usage.output_tokens,
            duration_ms = started.elapsed().as_millis() as u64,
            stop_reason = format!("{:?}", parsed.stop_reason)
        );

        Ok(parsed)
    }
}

#[async_trait]
impl Provider for OpenRouterProvider {
    #[tracing::instrument(skip_all, fields(provider = "openrouter", model = %request.model))]
    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, ProviderError> {
        let max_attempts = usize::from(self.empty_response_retries) + 1;
        for attempt in 0..max_attempts {
            logfire::info!(
                "provider chat attempt",
                provider = "openrouter",
                model = request.model.clone(),
                attempt = (attempt + 1) as u64,
                max_attempts = max_attempts as u64
            );
            match self.send_request(&request).await {
                Ok(response) => return Ok(response),
                Err(ProviderError::EmptyResponse) if attempt + 1 < max_attempts => {
                    logfire::warn!(
                        "provider returned empty response; retrying",
                        provider = "openrouter",
                        model = request.model.clone(),
                        attempt = attempt + 1
                    );
                }
                Err(error) => return Err(error),
            }
        }

        logfire::error!(
            "provider exhausted empty-response retries",
            provider = "openrouter",
            model = request.model.clone(),
            max_attempts = max_attempts as u64
        );
        Err(ProviderError::EmptyResponse)
    }

    fn name(&self) -> &str {
        "openrouter"
    }
}

fn parse_retry_after_secs(retry_after: Option<&str>) -> Option<u64> {
    retry_after.and_then(|value| value.trim().parse::<u64>().ok())
}

async fn extract_error_message(response: reqwest::Response) -> String {
    let body = response.text().await.unwrap_or_default();
    if body.trim().is_empty() {
        return "empty error response".to_string();
    }

    if let Ok(error_body) = serde_json::from_str::<ProviderErrorBody>(&body)
        && let Some(payload) = error_body.error
        && let Some(message) = payload.message
        && !message.trim().is_empty()
    {
        return message;
    }

    body
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::net::SocketAddr;
    use std::sync::{Arc, Mutex};

    use serde_json::json;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    use super::*;
    use crate::providers::types::{ContentBlock, StopReason, user_message};

    #[tokio::test]
    async fn maps_rate_limits_to_provider_error() {
        let server = MockServer::start(vec![MockResponse::json(
            429,
            Some(vec![("Retry-After".to_string(), "9".to_string())]),
            json!({"error": {"message": "rate limited"}}),
        )])
        .await;

        let provider = OpenRouterProvider::new_for_tests(server.url(), 0);
        let request = request_without_tools();
        let error = provider.chat(request).await.expect_err("expected 429");
        match error {
            ProviderError::RateLimited { retry_after_secs } => {
                assert_eq!(retry_after_secs, Some(9));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[tokio::test]
    async fn retries_empty_response() {
        let server = MockServer::start(vec![
            MockResponse::json(
                200,
                None,
                json!({
                    "model": "moonshotai/kimi-k2.5",
                    "choices": [{"message": {"role": "assistant", "content": ""}, "finish_reason": "stop"}]
                }),
            ),
            MockResponse::json(
                200,
                None,
                json!({
                    "model": "moonshotai/kimi-k2.5",
                    "choices": [{"message": {"role": "assistant", "content": "ok"}, "finish_reason": "stop"}],
                    "usage": {"prompt_tokens": 2, "completion_tokens": 1}
                }),
            ),
        ])
        .await;

        let provider = OpenRouterProvider::new_for_tests(server.url(), 1);
        let response = provider
            .chat(request_without_tools())
            .await
            .expect("second response should succeed");

        assert_eq!(response.stop_reason, StopReason::EndTurn);
        assert_eq!(response.content, vec![ContentBlock::Text("ok".to_string())]);
    }

    #[tokio::test]
    async fn circuit_breaker_opens_after_consecutive_failures() {
        let server = MockServer::start(vec![
            MockResponse::json(500, None, json!({"error": {"message": "boom"}})),
            MockResponse::json(500, None, json!({"error": {"message": "boom"}})),
        ])
        .await;

        let provider = OpenRouterProvider::new_for_tests(server.url(), 0);
        let request = request_without_tools();
        let _ = provider
            .chat(request.clone())
            .await
            .expect_err("first failure");
        let _ = provider
            .chat(request.clone())
            .await
            .expect_err("second failure");

        let error = provider
            .chat(request)
            .await
            .expect_err("circuit should be open");
        assert!(matches!(error, ProviderError::CircuitOpen { .. }));
    }

    fn request_without_tools() -> ChatRequest {
        ChatRequest {
            model: "moonshotai/kimi-k2.5".to_string(),
            messages: vec![user_message("say hello")],
            tools: None,
            max_tokens: Some(32),
            temperature: Some(0.0),
            system: None,
        }
    }

    struct MockResponse {
        status: u16,
        headers: Vec<(String, String)>,
        body: String,
    }

    impl MockResponse {
        fn json(
            status: u16,
            headers: Option<Vec<(String, String)>>,
            body: serde_json::Value,
        ) -> Self {
            Self {
                status,
                headers: headers.unwrap_or_default(),
                body: body.to_string(),
            }
        }
    }

    struct MockServer {
        address: SocketAddr,
        responses: Arc<Mutex<VecDeque<MockResponse>>>,
    }

    impl MockServer {
        async fn start(responses: Vec<MockResponse>) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind mock server");
            let address = listener.local_addr().expect("local addr");
            let responses = Arc::new(Mutex::new(VecDeque::from(responses)));
            let responses_for_task = Arc::clone(&responses);

            tokio::spawn(async move {
                while let Ok((socket, _)) = listener.accept().await {
                    if handle_socket(socket, &responses_for_task).await.is_err() {
                        break;
                    }
                }
            });

            Self { address, responses }
        }

        fn url(&self) -> String {
            format!("http://{}/chat/completions", self.address)
        }
    }

    impl Drop for MockServer {
        fn drop(&mut self) {
            let mut responses = self.responses.lock().expect("lock responses");
            responses.clear();
        }
    }

    async fn handle_socket(
        mut socket: TcpStream,
        responses: &Arc<Mutex<VecDeque<MockResponse>>>,
    ) -> Result<(), std::io::Error> {
        let mut buffer = vec![0_u8; 16 * 1024];
        let _bytes_read = socket.read(&mut buffer).await?;

        let response = {
            let mut guard = responses.lock().expect("lock responses");
            guard.pop_front().unwrap_or_else(|| MockResponse {
                status: 500,
                headers: vec![],
                body: "{}".to_string(),
            })
        };

        let status_text = match response.status {
            200 => "OK",
            401 => "Unauthorized",
            403 => "Forbidden",
            404 => "Not Found",
            429 => "Too Many Requests",
            500 => "Internal Server Error",
            _ => "Error",
        };
        let mut headers = format!(
            "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",
            response.status,
            status_text,
            response.body.len()
        );
        for (name, value) in response.headers {
            headers.push_str(&format!("{name}: {value}\r\n"));
        }
        headers.push_str("\r\n");
        socket.write_all(headers.as_bytes()).await?;
        socket.write_all(response.body.as_bytes()).await?;
        socket.flush().await?;
        Ok(())
    }
}
