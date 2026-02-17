use std::collections::BTreeMap;

use ghost::providers::{
    ChatRequest, ContentBlock, KimiCodeProvider, Provider, ResponseFormat, StopReason, user_message,
};

#[tokio::test]
async fn kimi_code_live_chat_completion_returns_text() {
    let _observability =
        ghost::observability::init_for_live_tests().expect("init live test observability");

    if std::env::var("KIMI_API_KEY")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .is_none()
    {
        eprintln!("KIMI_API_KEY not set; skipping Kimi Code live test.");
        return;
    }

    let provider = KimiCodeProvider::new(BTreeMap::new()).expect("KIMI_API_KEY must be set");
    let request = ChatRequest {
        model: "kimi-k2-0711-chat".to_string(),
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
async fn kimi_code_live_chat_completion_with_response_format_returns_json() {
    let _observability =
        ghost::observability::init_for_live_tests().expect("init live test observability");

    if std::env::var("KIMI_API_KEY")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .is_none()
    {
        eprintln!("KIMI_API_KEY not set; skipping Kimi Code live test.");
        return;
    }

    let provider = KimiCodeProvider::new(BTreeMap::new()).expect("KIMI_API_KEY must be set");
    let request = ChatRequest {
        model: "kimi-k2-0711-chat".to_string(),
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
