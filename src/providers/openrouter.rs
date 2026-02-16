use std::collections::BTreeMap;

use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue};

use crate::providers::openai_compatible_provider::OpenAiCompatibleProvider;
use crate::providers::types::{ChatRequest, ChatResponse, Provider, ProviderError};

const OPENROUTER_CHAT_COMPLETIONS_URL: &str = "https://openrouter.ai/api/v1/chat/completions";
const OPENROUTER_API_KEY_ENV: &str = "OPENROUTER_API_KEY";
const DEFAULT_EMPTY_RESPONSE_RETRIES: u8 = 2;

#[derive(Debug)]
pub struct OpenRouterProvider {
    inner: OpenAiCompatibleProvider,
}

impl OpenRouterProvider {
    #[tracing::instrument(skip_all)]
    pub fn new(extra_headers: BTreeMap<String, String>) -> Result<Self, ProviderError> {
        let mut headers = HeaderMap::new();
        headers.insert("X-Title", HeaderValue::from_static("ghost"));

        let inner = OpenAiCompatibleProvider::with_auth_env(
            "openrouter",
            OPENROUTER_CHAT_COMPLETIONS_URL,
            OPENROUTER_API_KEY_ENV,
            headers,
            extra_headers,
            DEFAULT_EMPTY_RESPONSE_RETRIES,
        )?;
        Ok(Self { inner })
    }

    #[cfg(test)]
    fn new_for_tests(endpoint: impl Into<String>, empty_response_retries: u8) -> Self {
        Self {
            inner: OpenAiCompatibleProvider::new_for_tests(
                "openrouter",
                endpoint,
                empty_response_retries,
            ),
        }
    }
}

#[async_trait]
impl Provider for OpenRouterProvider {
    #[tracing::instrument(skip_all, fields(provider = "openrouter", model = %request.model))]
    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, ProviderError> {
        self.inner.chat(request).await
    }

    fn name(&self) -> &str {
        self.inner.name()
    }
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
            response_format: None,
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
