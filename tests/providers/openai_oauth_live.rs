use std::collections::BTreeMap;

use ghost::providers::{
    ChatMessage, ChatRequest, ContentBlock, OpenAiOAuthProvider, Provider, Role, StopReason,
    ToolDefinition, user_message,
};
use ghost::tools::ToolManager;

fn respond_tool_schema() -> Vec<ToolDefinition> {
    ToolManager::for_chat()
        .all_tool_schemas()
        .into_iter()
        .filter(|t| t.name == "respond")
        .collect()
}

#[tokio::test]
async fn openai_oauth_live_calls_respond_tool() {
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
        tools: Some(respond_tool_schema()),
        max_tokens: None,
        temperature: None,
        system: Some(
            "You are a precise assistant. You MUST call the respond tool to deliver your answer."
                .to_string(),
        ),
    };

    let response = provider
        .chat(request)
        .await
        .expect("provider chat response");
    assert_eq!(
        response.stop_reason,
        StopReason::ToolUse,
        "expected ToolUse stop reason, got {:?}.\nContent: {:#?}",
        response.stop_reason,
        response.content,
    );

    let respond_call = response
        .content
        .iter()
        .find_map(|block| match block {
            ContentBlock::ToolUse { name, input, .. } if name == "respond" => Some(input),
            _ => None,
        })
        .expect("expected respond tool call");
    assert!(
        respond_call
            .get("message")
            .and_then(|v| v.as_str())
            .is_some(),
        "respond tool input must contain 'message', got: {respond_call}",
    );
}

/// Two-turn exchange where the first response's full content (including any
/// RawOutput items like reasoning) is fed back as history. Validates that
/// reasoning items survive the round-trip.
#[tokio::test]
async fn openai_oauth_multi_turn_with_history() {
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

    // Turn 1
    let request1 = ChatRequest {
        model: model.clone(),
        messages: vec![user_message("What is 2 + 2? Reply in one word.")],
        tools: None,
        max_tokens: None,
        temperature: None,
        system: Some("You are a concise assistant.".to_string()),
    };

    let response1 = provider.chat(request1).await.expect("turn 1 chat response");

    let has_usable = response1
        .content
        .iter()
        .any(|b| !matches!(b, ContentBlock::RawOutput { .. }));
    assert!(
        has_usable,
        "turn 1 should have usable content, got: {:#?}",
        response1.content,
    );

    let raw_count = response1
        .content
        .iter()
        .filter(|b| matches!(b, ContentBlock::RawOutput { .. }))
        .count();
    eprintln!(
        "Turn 1: {} content blocks ({} raw output items)",
        response1.content.len(),
        raw_count,
    );

    // Turn 2 — echo back the full content (including RawOutput) as history.
    let mut messages = vec![
        user_message("What is 2 + 2? Reply in one word."),
        ChatMessage {
            role: Role::Assistant,
            content: response1.content,
        },
        user_message("Now what is 3 + 3? Reply in one word."),
    ];

    let request2 = ChatRequest {
        model,
        messages: messages.clone(),
        tools: None,
        max_tokens: None,
        temperature: None,
        system: Some("You are a concise assistant.".to_string()),
    };

    let response2 = provider.chat(request2).await.expect("turn 2 chat response");

    let has_usable_2 = response2
        .content
        .iter()
        .any(|b| !matches!(b, ContentBlock::RawOutput { .. }));
    assert!(
        has_usable_2,
        "turn 2 should have usable content, got: {:#?}",
        response2.content,
    );

    // Suppress unused variable warning.
    messages.clear();
}
