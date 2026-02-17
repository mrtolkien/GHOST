use std::collections::BTreeMap;

use ghost::providers::{
    ChatRequest, ContentBlock, OpenAiOAuthProvider, Provider, StopReason, ToolDefinition,
    user_message,
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
