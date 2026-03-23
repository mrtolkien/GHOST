use std::fmt;
use std::sync::Arc;

use async_trait::async_trait;

use crate::providers::types::{ChatRequest, ChatResponse, Provider, ProviderError};

/// A provider that tries multiple inner providers in order.
///
/// On retryable errors (rate limit, server error, timeout, circuit open,
/// empty response, invalid response), falls through to the next provider.
/// On permanent errors (auth, model not found, context overflow), stops
/// immediately.
pub struct ChainProvider {
    /// (alias, provider, model_name) tuples in fallback order.
    providers: Vec<(String, Arc<dyn Provider>, String)>,
}

impl fmt::Debug for ChainProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let entries: Vec<(&str, &str)> = self
            .providers
            .iter()
            .map(|(alias, _, model)| (alias.as_str(), model.as_str()))
            .collect();
        f.debug_struct("ChainProvider")
            .field("providers", &entries)
            .finish()
    }
}

impl ChainProvider {
    #[must_use]
    pub fn new(providers: Vec<(String, Arc<dyn Provider>, String)>) -> Self {
        assert!(
            !providers.is_empty(),
            "ChainProvider requires at least one provider"
        );
        Self { providers }
    }
}

#[async_trait]
impl Provider for ChainProvider {
    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, ProviderError> {
        let mut errors = Vec::new();

        for (i, (alias, provider, model_name)) in self.providers.iter().enumerate() {
            let mut req = request.clone();
            req.model = model_name.clone();

            match provider.chat(req).await {
                Ok(response) => return Ok(response),
                Err(err) => {
                    let is_permanent = matches!(
                        &err,
                        ProviderError::Auth(_)
                            | ProviderError::ModelNotFound(_)
                            | ProviderError::ContextOverflow(_)
                    );

                    if is_permanent {
                        return Err(err);
                    }

                    let is_last = i == self.providers.len() - 1;
                    if !is_last {
                        let next_alias = &self.providers[i + 1].0;
                        tracing::info!(
                            model = alias.as_str(),
                            error = %err,
                            next = next_alias.as_str(),
                            "model failed, trying next in chain",
                        );
                    }

                    errors.push((alias.clone(), Box::new(err)));
                }
            }
        }

        Err(ProviderError::ChainExhausted { errors })
    }

    fn name(&self) -> &str {
        self.providers
            .first()
            .map(|(_, p, _)| p.name())
            .unwrap_or("chain")
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::providers::types::{ChatMessage, ContentBlock, Role, StopReason, Usage};

    /// A fake provider that returns queued results in order.
    #[derive(Debug)]
    struct FakeProvider {
        provider_name: String,
        results: Mutex<Vec<Result<ChatResponse, ProviderError>>>,
    }

    impl FakeProvider {
        fn new(name: &str, results: Vec<Result<ChatResponse, ProviderError>>) -> Arc<Self> {
            Arc::new(Self {
                provider_name: name.to_string(),
                // Reverse so we can pop from the back.
                results: Mutex::new(results.into_iter().rev().collect()),
            })
        }
    }

    #[async_trait]
    impl Provider for FakeProvider {
        async fn chat(&self, _request: ChatRequest) -> Result<ChatResponse, ProviderError> {
            self.results
                .lock()
                .expect("test mutex poisoned")
                .pop()
                .expect("FakeProvider ran out of queued results")
        }

        fn name(&self) -> &str {
            &self.provider_name
        }
    }

    fn ok_response(text: &str) -> ChatResponse {
        ChatResponse {
            content: vec![ContentBlock::Text {
                text: text.to_string(),
            }],
            usage: Usage::default(),
            stop_reason: StopReason::EndTurn,
            model: "test-model".to_string(),
            response_id: None,
            turn_state: None,
        }
    }

    fn default_request() -> ChatRequest {
        ChatRequest {
            model: "ignored".to_string(),
            messages: vec![ChatMessage {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: "hello".to_string(),
                }],
            }],
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn chain_uses_first_provider_on_success() {
        let p = FakeProvider::new("first", vec![Ok(ok_response("hi"))]);
        let chain = ChainProvider::new(vec![(
            "model-a".to_string(),
            p as Arc<dyn Provider>,
            "m1".to_string(),
        )]);

        let resp = chain.chat(default_request()).await.unwrap();
        assert_eq!(resp.content.len(), 1);
    }

    #[tokio::test]
    async fn chain_falls_through_on_rate_limit() {
        let p1 = FakeProvider::new(
            "first",
            vec![Err(ProviderError::RateLimited {
                retry_after_secs: Some(5),
            })],
        );
        let p2 = FakeProvider::new("second", vec![Ok(ok_response("ok"))]);

        let chain = ChainProvider::new(vec![
            (
                "model-a".to_string(),
                p1 as Arc<dyn Provider>,
                "m1".to_string(),
            ),
            (
                "model-b".to_string(),
                p2 as Arc<dyn Provider>,
                "m2".to_string(),
            ),
        ]);

        let resp = chain.chat(default_request()).await.unwrap();
        assert_eq!(resp.content.len(), 1);
    }

    #[tokio::test]
    async fn chain_stops_on_auth_error() {
        let p1 = FakeProvider::new(
            "first",
            vec![Err(ProviderError::Auth("bad key".to_string()))],
        );
        let p2 = FakeProvider::new("second", vec![Ok(ok_response("ok"))]);

        let chain = ChainProvider::new(vec![
            (
                "model-a".to_string(),
                p1 as Arc<dyn Provider>,
                "m1".to_string(),
            ),
            (
                "model-b".to_string(),
                p2 as Arc<dyn Provider>,
                "m2".to_string(),
            ),
        ]);

        let err = chain.chat(default_request()).await.unwrap_err();
        assert!(
            matches!(err, ProviderError::Auth(_)),
            "expected Auth, got: {err:?}"
        );
    }

    #[tokio::test]
    async fn chain_stops_on_context_overflow() {
        let p1 = FakeProvider::new(
            "first",
            vec![Err(ProviderError::ContextOverflow("too long".to_string()))],
        );
        let p2 = FakeProvider::new("second", vec![Ok(ok_response("ok"))]);

        let chain = ChainProvider::new(vec![
            (
                "model-a".to_string(),
                p1 as Arc<dyn Provider>,
                "m1".to_string(),
            ),
            (
                "model-b".to_string(),
                p2 as Arc<dyn Provider>,
                "m2".to_string(),
            ),
        ]);

        let err = chain.chat(default_request()).await.unwrap_err();
        assert!(
            matches!(err, ProviderError::ContextOverflow(_)),
            "expected ContextOverflow, got: {err:?}"
        );
    }

    #[tokio::test]
    async fn chain_exhausted_collects_all_errors() {
        let p1 = FakeProvider::new(
            "first",
            vec![Err(ProviderError::RateLimited {
                retry_after_secs: None,
            })],
        );
        let p2 = FakeProvider::new("second", vec![Err(ProviderError::Timeout { seconds: 30 })]);

        let chain = ChainProvider::new(vec![
            (
                "model-a".to_string(),
                p1 as Arc<dyn Provider>,
                "m1".to_string(),
            ),
            (
                "model-b".to_string(),
                p2 as Arc<dyn Provider>,
                "m2".to_string(),
            ),
        ]);

        let err = chain.chat(default_request()).await.unwrap_err();
        match err {
            ProviderError::ChainExhausted { errors } => {
                assert_eq!(errors.len(), 2);
                assert_eq!(errors[0].0, "model-a");
                assert_eq!(errors[1].0, "model-b");
            }
            other => panic!("expected ChainExhausted, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn chain_name_returns_first_provider() {
        let p = FakeProvider::new("my-provider", vec![]);
        let chain = ChainProvider::new(vec![(
            "alias".to_string(),
            p as Arc<dyn Provider>,
            "m".to_string(),
        )]);

        assert_eq!(chain.name(), "my-provider");
    }
}
