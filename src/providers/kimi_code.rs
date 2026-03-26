use std::collections::BTreeMap;

use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue, USER_AGENT};

use crate::providers::openai_compatible_provider::OpenAiCompatibleProvider;
use crate::providers::types::{ChatRequest, ChatResponse, Provider, ProviderError};

const KIMI_CHAT_COMPLETIONS_URL: &str = "https://api.kimi.com/coding/v1/chat/completions";
const KIMI_API_KEY_ENV: &str = "KIMI_API_KEY";
const DEFAULT_KIMI_USER_AGENT: &str = "KimiCLI/1.12.0";

#[derive(Debug)]
pub struct KimiCodeProvider {
    inner: OpenAiCompatibleProvider,
}

impl KimiCodeProvider {
    #[tracing::instrument(skip_all)]
    pub fn new(extra_headers: BTreeMap<String, String>) -> Result<Self, ProviderError> {
        let mut headers = HeaderMap::new();
        headers.insert(
            USER_AGENT,
            HeaderValue::from_static(DEFAULT_KIMI_USER_AGENT),
        );

        let inner = OpenAiCompatibleProvider::with_auth_env(
            "kimi_code",
            KIMI_CHAT_COMPLETIONS_URL,
            KIMI_API_KEY_ENV,
            headers,
            extra_headers,
            None,
        )?;
        Ok(Self { inner })
    }

    pub fn set_debug(&mut self, save: bool, workspace: &std::path::Path, max_saved: usize) {
        self.inner.set_debug(save, workspace, max_saved);
    }
}

#[async_trait]
impl Provider for KimiCodeProvider {
    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, ProviderError> {
        self.inner.chat(request).await
    }

    fn name(&self) -> &str {
        self.inner.name()
    }
}
