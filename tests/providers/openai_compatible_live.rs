use std::collections::BTreeMap;

use ghost::providers::{ChatRequest, ContentBlock, Provider, StopReason, user_message};
use reqwest::header::HeaderMap;

const LOCAL_OPENAI_COMPATIBLE_URL: &str = "http://192.168.1.91:11434/v1/chat/completions";
const LOCAL_OPENAI_COMPATIBLE_MODEL: &str = "gemma4:26b";

#[tokio::test]
async fn openai_compatible_gemma4_local_smoke() {
    let _observability =
        ghost::observability::init_for_live_tests().expect("init live test observability");

    let provider =
        ghost::providers::openai_compatible_provider::OpenAiCompatibleProvider::without_auth(
            "openai_compatible",
            LOCAL_OPENAI_COMPATIBLE_URL,
            HeaderMap::new(),
            BTreeMap::new(),
            None,
        )
        .expect("OpenAI-compatible provider init");

    let response = provider
        .chat(ChatRequest {
            model: LOCAL_OPENAI_COMPATIBLE_MODEL.into(),
            messages: vec![user_message("Reply with exactly: OK")],
            tools: None,
            max_tokens: Some(512),
            temperature: Some(0.0),
            system: Some("You are a test assistant.".into()),
            reasoning_effort: None,
            cache_key: String::new(),
            turn_state: None,
            debug_context: None,
            text_verbosity: None,
        })
        .await
        .expect("chat request");

    let text = response
        .content
        .iter()
        .find_map(|block| match block {
            ContentBlock::Text { text } => Some(text.trim()),
            _ => None,
        })
        .expect("response should contain a text block");

    assert_eq!(text, "OK");
    assert_eq!(response.stop_reason, StopReason::EndTurn);
    assert!(response.usage.input_tokens > 0);
}
