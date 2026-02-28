use std::collections::BTreeMap;

use ghost::providers::{
    ChatMessage, ChatRequest, ContentBlock, OpenAiOAuthProvider, Provider, ReasoningEffort, Role,
    user_message,
};

/// Validates that reasoning effort "high" produces thinking blocks in the
/// response, and that those blocks survive a round-trip (echo back as history).
///
/// ```sh
/// cargo test --features live-tests reasoning_effort_high -- --nocapture
/// ```
#[tokio::test]
async fn reasoning_effort_high_produces_thinking_block() {
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

    // Turn 1: ask a question that requires reasoning.
    let request = ChatRequest {
        model: "gpt-5.3-codex".to_string(),
        messages: vec![user_message(
            "What is the sum of the first 20 prime numbers? Show your work briefly.",
        )],
        tools: None,
        max_tokens: None,
        temperature: None,
        system: Some("You are a concise math assistant.".to_string()),
        reasoning_effort: Some(ReasoningEffort::High),
        cache_key: "reasoning-test".to_string(),
        turn_state: None,
        debug_context: None,
    };

    let response = provider.chat(request).await.expect("turn 1 chat response");

    eprintln!("Turn 1 content blocks: {}", response.content.len());
    for (i, block) in response.content.iter().enumerate() {
        match block {
            ContentBlock::RawOutput {
                original_type,
                value,
            } => {
                let has_encrypted = value.get("encrypted_content").is_some();
                let summary_len = value
                    .get("summary")
                    .and_then(|s| s.as_array())
                    .map_or(0, |a| a.len());
                eprintln!(
                    "  [{i}] RawOutput({original_type}): encrypted={has_encrypted}, \
                     summary_parts={summary_len}"
                );
            }
            ContentBlock::Text { text } => {
                eprintln!("  [{i}] Text: {} chars", text.len());
            }
            other => {
                eprintln!("  [{i}] {other:?}");
            }
        }
    }

    // Assert: response contains a reasoning block.
    let reasoning_block = response
        .content
        .iter()
        .find(|b| {
            matches!(
                b,
                ContentBlock::RawOutput { original_type, .. }
                    if original_type == "reasoning"
            )
        })
        .expect("response must contain a reasoning RawOutput block");

    // Assert: the reasoning block has encrypted_content (needed for echo-back).
    if let ContentBlock::RawOutput { value, .. } = reasoning_block {
        assert!(
            value.get("encrypted_content").is_some(),
            "reasoning block must contain encrypted_content for round-trip"
        );
    }

    // Assert: response also has usable text content.
    let has_text = response
        .content
        .iter()
        .any(|b| matches!(b, ContentBlock::Text { .. }));
    assert!(has_text, "response must also contain a Text block");

    // Turn 2: echo back the full response (including reasoning) as history.
    let request2 = ChatRequest {
        model: "gpt-5.3-codex".to_string(),
        messages: vec![
            user_message("What is the sum of the first 20 prime numbers? Show your work briefly."),
            ChatMessage {
                role: Role::Assistant,
                content: response.content,
            },
            user_message("Now what is the sum of the first 10 prime numbers?"),
        ],
        tools: None,
        max_tokens: None,
        temperature: None,
        system: Some("You are a concise math assistant.".to_string()),
        reasoning_effort: Some(ReasoningEffort::High),
        cache_key: "reasoning-test".to_string(),
        turn_state: None,
        debug_context: None,
    };

    let response2 = provider
        .chat(request2)
        .await
        .expect("turn 2 chat response (reasoning round-trip)");

    eprintln!("Turn 2 content blocks: {}", response2.content.len());
    for (i, block) in response2.content.iter().enumerate() {
        match block {
            ContentBlock::RawOutput { original_type, .. } => {
                eprintln!("  [{i}] RawOutput({original_type})")
            }
            ContentBlock::Text { text } => eprintln!("  [{i}] Text: {} chars", text.len()),
            other => eprintln!("  [{i}] {other:?}"),
        }
    }

    // Turn 2 should also succeed and contain text.
    let has_text_2 = response2
        .content
        .iter()
        .any(|b| matches!(b, ContentBlock::Text { .. }));
    assert!(
        has_text_2,
        "turn 2 response must contain text (reasoning echo-back works)"
    );
}
