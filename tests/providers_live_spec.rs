#![cfg(feature = "live-tests")]

use std::collections::BTreeMap;

use ghost::providers::{
    ChatRequest, ContentBlock, OpenRouterProvider, Provider, StopReason, user_message,
};

#[tokio::test]
async fn openrouter_live_chat_completion_returns_text() {
    let _observability =
        ghost::observability::init_for_live_tests().expect("init live test observability");

    let provider =
        OpenRouterProvider::new(BTreeMap::new()).expect("OPENROUTER_API_KEY must be set");
    let request = ChatRequest {
        model: "moonshotai/kimi-k2.5".to_string(),
        messages: vec![user_message(
            "Reply with exactly one short sentence about Rust programming.",
        )],
        tools: None,
        max_tokens: None,
        temperature: None,
        system: Some("You are a precise assistant.".to_string()),
    };

    let response = provider
        .chat(request)
        .await
        .expect("provider chat response");
    assert!(matches!(
        response.stop_reason,
        StopReason::EndTurn | StopReason::MaxTokens
    ));
    assert!(
        response
            .content
            .iter()
            .any(|block| matches!(block, ContentBlock::Text(text) if !text.trim().is_empty()))
    );
}
