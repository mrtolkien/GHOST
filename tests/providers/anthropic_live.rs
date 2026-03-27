use std::collections::BTreeMap;

use ghost::providers::{
    AnthropicProvider, ChatMessage, ChatRequest, ContentBlock, Provider, ReasoningEffort, Role,
    StopReason, ToolDefinition, user_message,
};
use ghost::tools::manager::ToolManager;
use serde_json::json;

#[tokio::test]
async fn anthropic_simple_chat() {
    let _observability =
        ghost::observability::init_for_live_tests().expect("init live test observability");

    let provider = match AnthropicProvider::new(BTreeMap::new()) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Skipping: no Claude Code credentials ({e})");
            return;
        }
    };

    let response = provider
        .chat(ChatRequest {
            model: "claude-sonnet-4-6".into(),
            messages: vec![user_message("Say 'hello' and nothing else.")],
            tools: None,
            max_tokens: Some(100),
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

    assert!(!response.content.is_empty(), "response should have content");
    let text = response
        .content
        .iter()
        .find_map(|b| match b {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .expect("should have text block");
    assert!(
        text.to_lowercase().contains("hello"),
        "expected 'hello' in response, got: {text}"
    );
    assert_eq!(response.stop_reason, StopReason::EndTurn);
    assert!(response.usage.input_tokens > 0);
    assert!(response.usage.output_tokens > 0);
}

#[tokio::test]
async fn anthropic_tool_use() {
    let _observability =
        ghost::observability::init_for_live_tests().expect("init live test observability");

    let provider = match AnthropicProvider::new(BTreeMap::new()) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Skipping: no Claude Code credentials ({e})");
            return;
        }
    };

    let response = provider
        .chat(ChatRequest {
            model: "claude-sonnet-4-6".into(),
            messages: vec![user_message(
                "What's the weather in Paris? Use the get_weather tool.",
            )],
            tools: Some(vec![ToolDefinition {
                name: "get_weather".into(),
                description: "Get the weather for a city.".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "city": {"type": "string"}
                    },
                    "required": ["city"]
                }),
            }]),
            max_tokens: Some(1024),
            temperature: None,
            system: None,
            reasoning_effort: None,
            cache_key: String::new(),
            turn_state: None,
            debug_context: None,
            text_verbosity: None,
        })
        .await
        .expect("chat request");

    assert_eq!(response.stop_reason, StopReason::ToolUse);
    let tool_use = response
        .content
        .iter()
        .find(|b| matches!(b, ContentBlock::ToolUse { .. }))
        .expect("should have tool_use block");
    match tool_use {
        ContentBlock::ToolUse { name, input, .. } => {
            assert_eq!(name, "get_weather");
            assert!(input.get("city").is_some());
        }
        _ => unreachable!(),
    }
}

#[tokio::test]
async fn anthropic_multi_turn_with_history() {
    let _observability =
        ghost::observability::init_for_live_tests().expect("init live test observability");

    let provider = match AnthropicProvider::new(BTreeMap::new()) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Skipping: no Claude Code credentials ({e})");
            return;
        }
    };

    // Turn 1
    let response1 = provider
        .chat(ChatRequest {
            model: "claude-sonnet-4-6".into(),
            messages: vec![user_message("What is 2 + 2? Reply in one word.")],
            tools: None,
            max_tokens: Some(100),
            temperature: Some(0.0),
            system: Some("You are a concise assistant.".into()),
            reasoning_effort: None,
            cache_key: String::new(),
            turn_state: None,
            debug_context: None,
            text_verbosity: None,
        })
        .await
        .expect("turn 1 chat response");

    assert!(
        response1.content.iter().any(|b| !matches!(
            b,
            ContentBlock::RawOutput { .. } | ContentBlock::Thinking { .. }
        )),
        "turn 1 should have usable content"
    );

    // Turn 2 — echo back full content as history
    let response2 = provider
        .chat(ChatRequest {
            model: "claude-sonnet-4-6".into(),
            messages: vec![
                user_message("What is 2 + 2? Reply in one word."),
                ChatMessage {
                    role: Role::Assistant,
                    content: response1.content,
                },
                user_message("Now what is 3 + 3? Reply in one word."),
            ],
            tools: None,
            max_tokens: Some(100),
            temperature: Some(0.0),
            system: Some("You are a concise assistant.".into()),
            reasoning_effort: None,
            cache_key: String::new(),
            turn_state: None,
            debug_context: None,
            text_verbosity: None,
        })
        .await
        .expect("turn 2 chat response");

    assert!(
        response2.content.iter().any(|b| !matches!(
            b,
            ContentBlock::RawOutput { .. } | ContentBlock::Thinking { .. }
        )),
        "turn 2 should have usable content"
    );
}

/// Validates that the Anthropic OAuth endpoint accepts all of Ghost's
/// tools — including non-Claude-Code ones like `knowledge_search`,
/// `note_write`, `agent`, etc. — and can produce a tool call for one.
#[tokio::test]
async fn anthropic_tool_use_with_full_ghost_toolset() {
    let _observability =
        ghost::observability::init_for_live_tests().expect("init live test observability");

    let provider = match AnthropicProvider::new(BTreeMap::new()) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Skipping: no Claude Code credentials ({e})");
            return;
        }
    };

    let all_tools = ToolManager::all_available().all_tool_schemas();
    let tool_count = all_tools.len();
    assert!(
        tool_count >= 8,
        "expected at least 8 tools, got {tool_count}"
    );

    let tool_names: Vec<&str> = all_tools.iter().map(|t| t.name.as_str()).collect();
    eprintln!("Sending {tool_count} tools: {tool_names:?}");

    let response = provider
        .chat(ChatRequest {
            model: "claude-sonnet-4-6".into(),
            messages: vec![user_message(
                "Search my knowledge base for notes about Rust. Use the knowledge_search tool.",
            )],
            tools: Some(all_tools),
            max_tokens: Some(1024),
            temperature: Some(0.0),
            system: Some("You are a helpful assistant.".into()),
            reasoning_effort: None,
            cache_key: String::new(),
            turn_state: None,
            debug_context: None,
            text_verbosity: None,
        })
        .await
        .expect("chat request with full Ghost toolset");

    assert_eq!(
        response.stop_reason,
        StopReason::ToolUse,
        "expected tool_use stop reason, got {:?}",
        response.stop_reason
    );

    let tool_use = response
        .content
        .iter()
        .find(|b| matches!(b, ContentBlock::ToolUse { .. }))
        .expect("should have a tool_use block");
    match tool_use {
        ContentBlock::ToolUse { name, .. } => {
            assert_eq!(name, "knowledge_search");
        }
        _ => unreachable!(),
    }
}

/// Validates that Anthropic thinking blocks are:
/// 1. Produced as `ContentBlock::Thinking` (not `RawOutput`)
/// 2. Contain text + signature
/// 3. Survive a round-trip (echo back as history in turn 2)
///
/// This exercises the critical ordering fix: thinking blocks must
/// precede tool_use/text in assistant messages per the Anthropic API.
///
/// ```sh
/// cargo test --features live-tests-llms anthropic_thinking -- --nocapture
/// ```
#[tokio::test]
async fn anthropic_thinking_block_round_trip() {
    let _observability =
        ghost::observability::init_for_live_tests().expect("init live test observability");

    let provider = match AnthropicProvider::new(BTreeMap::new()) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Skipping: no Claude Code credentials ({e})");
            return;
        }
    };

    // Turn 1: request with thinking enabled.
    let response1 = provider
        .chat(ChatRequest {
            model: "claude-sonnet-4-6".into(),
            messages: vec![user_message(
                "What is the sum of the first 10 prime numbers? Answer briefly.",
            )],
            tools: None,
            max_tokens: Some(2048),
            temperature: None,
            system: Some("You are a concise math assistant.".into()),
            reasoning_effort: Some(ReasoningEffort::High),
            cache_key: String::new(),
            turn_state: None,
            debug_context: None,
            text_verbosity: None,
        })
        .await
        .expect("turn 1 chat response");

    eprintln!("Turn 1 blocks: {}", response1.content.len());
    for (i, block) in response1.content.iter().enumerate() {
        match block {
            ContentBlock::Thinking {
                text, signature, ..
            } => {
                eprintln!(
                    "  [{i}] Thinking: text_len={}, has_sig={}",
                    text.as_ref().map_or(0, String::len),
                    signature.is_some(),
                );
            }
            ContentBlock::Text { text } => {
                eprintln!("  [{i}] Text: {} chars", text.len());
            }
            other => eprintln!("  [{i}] {other:?}"),
        }
    }

    // Assert: response contains a Thinking block with text + signature.
    let thinking = response1
        .content
        .iter()
        .find(|b| matches!(b, ContentBlock::Thinking { .. }))
        .expect("response must contain a Thinking block");

    if let ContentBlock::Thinking {
        text, signature, ..
    } = thinking
    {
        assert!(text.is_some(), "thinking block must have text");
        assert!(signature.is_some(), "thinking block must have signature");
    }

    // Assert: also has usable text content.
    assert!(
        response1
            .content
            .iter()
            .any(|b| matches!(b, ContentBlock::Text { .. })),
        "response must also contain a Text block"
    );

    // Turn 2: echo back the full response (including thinking) as
    // history. This validates that our thinking block ordering +
    // reconstruction is accepted by the Anthropic API.
    let response2 = provider
        .chat(ChatRequest {
            model: "claude-sonnet-4-6".into(),
            messages: vec![
                user_message("What is the sum of the first 10 prime numbers? Answer briefly."),
                ChatMessage {
                    role: Role::Assistant,
                    content: response1.content,
                },
                user_message("Now what is the sum of the first 5 prime numbers?"),
            ],
            tools: None,
            max_tokens: Some(2048),
            temperature: None,
            system: Some("You are a concise math assistant.".into()),
            reasoning_effort: Some(ReasoningEffort::High),
            cache_key: String::new(),
            turn_state: None,
            debug_context: None,
            text_verbosity: None,
        })
        .await
        .expect("turn 2 chat response (thinking round-trip)");

    eprintln!("Turn 2 blocks: {}", response2.content.len());
    for (i, block) in response2.content.iter().enumerate() {
        match block {
            ContentBlock::Thinking { .. } => eprintln!("  [{i}] Thinking"),
            ContentBlock::Text { text } => {
                eprintln!("  [{i}] Text: {} chars", text.len());
            }
            other => eprintln!("  [{i}] {other:?}"),
        }
    }

    // Turn 2 should succeed and have text content.
    assert!(
        response2
            .content
            .iter()
            .any(|b| matches!(b, ContentBlock::Text { .. })),
        "turn 2 must contain text (thinking round-trip works)"
    );
}
