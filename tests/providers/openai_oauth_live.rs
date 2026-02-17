use std::collections::BTreeMap;

use ghost::providers::{
    ChatRequest, ContentBlock, OpenAiOAuthProvider, Provider, ResponseFormat, StopReason,
    user_message,
};

#[tokio::test]
async fn openai_oauth_live_chat_completion_returns_text() {
    let _observability =
        ghost::observability::init_for_live_tests().expect("init live test observability");

    if ghost::auth::openai_oauth::auth_status()
        .await
        .expect("read oauth auth status")
        .is_none()
    {
        eprintln!("No OpenAI OAuth token found; run `ghost auth codex` first. Skipping.");
        return;
    }

    let provider =
        OpenAiOAuthProvider::new(BTreeMap::new()).expect("OpenAI OAuth provider initialization");
    let model =
        std::env::var("OPENAI_OAUTH_TEST_MODEL").unwrap_or_else(|_| "gpt-5-codex".to_string());
    let request = ChatRequest {
        model,
        messages: vec![user_message(
            "Reply with exactly one short sentence about Rust programming.",
        )],
        tools: None,
        max_tokens: None,
        temperature: None,
        system: Some("You are a precise assistant.".to_string()),
        response_format: None,
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
            .any(|block| matches!(block, ContentBlock::Text { text } if !text.trim().is_empty()))
    );
}

#[tokio::test]
async fn openai_oauth_live_chat_completion_with_response_format_returns_json() {
    let _observability =
        ghost::observability::init_for_live_tests().expect("init live test observability");

    if ghost::auth::openai_oauth::auth_status()
        .await
        .expect("read oauth auth status")
        .is_none()
    {
        eprintln!("No OpenAI OAuth token found; run `ghost auth codex` first. Skipping.");
        return;
    }

    let provider =
        OpenAiOAuthProvider::new(BTreeMap::new()).expect("OpenAI OAuth provider initialization");
    let model =
        std::env::var("OPENAI_OAUTH_TEST_MODEL").unwrap_or_else(|_| "gpt-5-codex".to_string());
    let request = ChatRequest {
        model,
        messages: vec![user_message(
            "Return a JSON object with field `answer` set to `ok`.",
        )],
        tools: None,
        max_tokens: None,
        temperature: None,
        system: Some("Return only valid JSON matching the schema.".to_string()),
        response_format: Some(ResponseFormat::JsonSchema {
            name: "short_answer".to_string(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "answer": { "type": "string" }
                },
                "required": ["answer"],
                "additionalProperties": false
            }),
        }),
    };

    let response = provider
        .chat(request)
        .await
        .expect("provider chat response");
    assert!(matches!(
        response.stop_reason,
        StopReason::EndTurn | StopReason::MaxTokens
    ));

    let first_text = response
        .content
        .iter()
        .find_map(|block| match block {
            ContentBlock::Text { text } if !text.trim().is_empty() => Some(text),
            _ => None,
        })
        .expect("expected text content");
    let parsed: serde_json::Value =
        serde_json::from_str(first_text).expect("expected valid json response");
    assert!(
        parsed
            .get("answer")
            .and_then(|value| value.as_str())
            .is_some()
    );
}
