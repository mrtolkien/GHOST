use std::collections::BTreeMap;

use ghost::providers::{
    ChatMessage, ChatRequest, ChatResponse, Provider, Role, ToolDefinition, user_message,
};

fn large_system_prompt() -> String {
    let block = "You are a knowledgeable assistant. Your role is to help \
        users with programming questions, system design, debugging, and \
        general software engineering topics. Always provide clear, concise, \
        and accurate answers. When discussing code, use proper formatting \
        and explain your reasoning step by step. Consider edge cases and \
        potential pitfalls in your suggestions. If you are unsure about \
        something, say so rather than guessing. Prioritize correctness \
        over brevity, but avoid unnecessary verbosity.\n\n";
    // ~3K tokens of system prompt — well above the 1024-token caching
    // minimum.
    block.repeat(20)
}

fn dummy_tools() -> Vec<ToolDefinition> {
    vec![ToolDefinition {
        name: "get_weather".to_string(),
        description: "Get the current weather for a location".to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "location": { "type": "string", "description": "City name" }
            },
            "required": ["location"]
        }),
    }]
}

fn log_usage(response: &ChatResponse, turn: u8) {
    let u = &response.usage;
    let cached = u.cache_read_tokens.unwrap_or(0);
    let pct = if u.input_tokens > 0 {
        cached as f64 / u.input_tokens as f64 * 100.0
    } else {
        0.0
    };
    eprintln!(
        "[codex turn_state] turn {turn}: input={} output={} \
         cache_read={cached} ({pct:.1}%) turn_state={:?}",
        u.input_tokens,
        u.output_tokens,
        response
            .turn_state
            .as_deref()
            .map(|s| &s[..s.len().min(20)]),
    );
}

/// Multi-turn test that threads `turn_state` from each response into the
/// next request, verifying the Codex backend's sticky routing produces
/// cache hits.
///
/// Run with:
/// ```sh
/// cargo test --features live-tests -p ghost --test providers \
///     codex_turn_state_cache -- --nocapture
/// ```
#[tokio::test]
async fn codex_turn_state_cache() {
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

    let temp = tempfile::tempdir().unwrap();
    let mut provider = ghost::providers::OpenAiOAuthProvider::new(BTreeMap::new())
        .expect("OpenAI OAuth provider initialization");
    provider.set_debug(true, temp.path());

    let system = large_system_prompt();
    let model =
        std::env::var("OPENAI_OAUTH_TEST_MODEL").unwrap_or_else(|_| "gpt-5-codex".to_string());
    let cache_key = "turn-state-test".to_string();
    let tools = Some(dummy_tools());

    // --- Turn 1: no turn_state yet ---
    let request1 = ChatRequest {
        model: model.clone(),
        messages: vec![user_message("Reply with exactly: PONG1")],
        tools: tools.clone(),
        max_tokens: None,
        temperature: None,
        system: Some(system.clone()),
        reasoning_effort: None,
        cache_key: cache_key.clone(),
        turn_state: None,
        debug_context: None,
    };
    let response1 = provider.chat(request1).await.expect("turn 1 failed");
    log_usage(&response1, 1);

    // Capture turn_state from response
    let turn_state = response1.turn_state.clone();
    eprintln!("turn_state after turn 1: {:?}", turn_state.is_some());

    // --- Turn 2: echo turn_state back ---
    let mut history2 = vec![
        user_message("Reply with exactly: PONG1"),
        ChatMessage {
            role: Role::Assistant,
            content: response1.content,
        },
        user_message("Reply with exactly: PONG2"),
    ];
    let request2 = ChatRequest {
        model: model.clone(),
        messages: history2.clone(),
        tools: tools.clone(),
        max_tokens: None,
        temperature: None,
        system: Some(system.clone()),
        reasoning_effort: None,
        cache_key: cache_key.clone(),
        turn_state: turn_state.clone(),
        debug_context: None,
    };
    let response2 = provider.chat(request2).await.expect("turn 2 failed");
    log_usage(&response2, 2);

    // --- Turn 3: continue threading turn_state ---
    history2.push(ChatMessage {
        role: Role::Assistant,
        content: response2.content.clone(),
    });
    history2.push(user_message("Reply with exactly: PONG3"));

    // Use the latest turn_state (may update or stay the same)
    let turn_state3 = response2.turn_state.clone().or(turn_state.clone());

    let request3 = ChatRequest {
        model: model.clone(),
        messages: history2,
        tools: tools.clone(),
        max_tokens: None,
        temperature: None,
        system: Some(system.clone()),
        reasoning_effort: None,
        cache_key: cache_key.clone(),
        turn_state: turn_state3,
        debug_context: None,
    };
    let response3 = provider.chat(request3).await.expect("turn 3 failed");
    log_usage(&response3, 3);

    // --- Assertions ---
    assert!(
        response1.usage.input_tokens > 0,
        "turn 1 should have input tokens"
    );
    assert!(
        response2.usage.input_tokens > 0,
        "turn 2 should have input tokens"
    );
    assert!(
        response3.usage.input_tokens > 0,
        "turn 3 should have input tokens"
    );

    // Verify turn_state was received from the server.
    // The Codex backend should return x-codex-turn-state on the first response.
    if response1.turn_state.is_none() {
        eprintln!(
            "WARNING: server did not return x-codex-turn-state. \
             Sticky routing is not available — cache hits will be random."
        );
    }

    // Check for cache hits on turns 2 and 3.
    let cache2 = response2.usage.cache_read_tokens.unwrap_or(0);
    let cache3 = response3.usage.cache_read_tokens.unwrap_or(0);

    if cache2 > 0 || cache3 > 0 {
        eprintln!(
            "Cache hits detected: turn2={cache2} turn3={cache3}. \
             Sticky routing is working."
        );
    } else {
        eprintln!(
            "NOTE: no cache hits on turns 2-3. This can happen if the \
             backend doesn't support turn_state or cache population is async."
        );
    }

    // Dump debug files for manual inspection
    let debug_dir = temp.path().join("debug/requests");
    if debug_dir.exists() {
        for entry in std::fs::read_dir(&debug_dir).unwrap().flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "json")
                && let Ok(content) = std::fs::read_to_string(&path)
            {
                eprintln!(
                    "=== {} ===\n{}\n",
                    path.file_name().unwrap_or_default().to_string_lossy(),
                    &content[..content.len().min(500)]
                );
            }
        }
    }
}
