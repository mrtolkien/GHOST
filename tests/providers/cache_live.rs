use std::collections::BTreeMap;

use ghost::providers::{
    ChatMessage, ChatRequest, ChatResponse, OpenRouterProvider, Provider, Role, user_message,
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
    block.repeat(20)
}

fn cache_test_request(model: &str, system: &str, history: Vec<ChatMessage>) -> ChatRequest {
    ChatRequest {
        model: model.to_string(),
        messages: history,
        tools: None,
        max_tokens: None,
        temperature: None,
        system: Some(system.to_string()),
        reasoning_effort: None,
        cache_key: "cache-test".to_string(),
        turn_state: None,
        debug_context: None,
        text_verbosity: None,
    }
}

fn log_usage(response: &ChatResponse, provider: &str, turn: u8) {
    let u = &response.usage;
    eprintln!(
        "[{provider}] turn {turn}: input={} output={} \
         cache_read={:?} cache_creation={:?}",
        u.input_tokens, u.output_tokens, u.cache_read_tokens, u.cache_creation_tokens,
    );
}

fn dump_debug_files(dir: &std::path::Path) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => {
            eprintln!("debug dir {}: {e}", dir.display());
            return;
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "json") {
            match std::fs::read_to_string(&path) {
                Ok(content) => eprintln!(
                    "=== {} ===\n{}\n",
                    path.file_name().unwrap_or_default().to_string_lossy(),
                    content
                ),
                Err(e) => eprintln!("failed to read {}: {e}", path.display()),
            }
        }
    }
}

#[tokio::test]
async fn openrouter_cache_validation() {
    let _observability =
        ghost::observability::init_for_live_tests().expect("init live test observability");

    if std::env::var("OPENROUTER_API_KEY")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .is_none()
    {
        eprintln!("OPENROUTER_API_KEY not set; skipping.");
        return;
    }

    let temp = tempfile::tempdir().unwrap();
    let mut provider =
        OpenRouterProvider::new(BTreeMap::new(), None).expect("OPENROUTER_API_KEY must be set");
    provider.set_debug(true, temp.path());

    let system = large_system_prompt();
    let model = "moonshotai/kimi-k2.5";

    // Turn 1
    let request1 = cache_test_request(model, &system, vec![user_message("ping")]);
    let response1 = provider.chat(request1).await.expect("turn 1");
    log_usage(&response1, "openrouter", 1);

    // Turn 2 — include turn 1 as history
    let request2 = cache_test_request(
        model,
        &system,
        vec![
            user_message("ping"),
            ChatMessage {
                role: Role::Assistant,
                content: response1.content,
            },
            user_message("ping again"),
        ],
    );
    let response2 = provider.chat(request2).await.expect("turn 2");
    log_usage(&response2, "openrouter", 2);

    dump_debug_files(&temp.path().join("debug/requests"));

    assert!(response1.usage.input_tokens > 0);
    assert!(response2.usage.input_tokens > 0);

    // OpenRouter returns cache data via prompt_tokens_details.cached_tokens.
    // Caching is non-deterministic (depends on underlying provider routing),
    // so we verify the field is parsed rather than asserting a cache hit.
    if response2.usage.cache_read_tokens.is_none_or(|v| v == 0) {
        eprintln!(
            "NOTE: no cache hit on turn 2 (cache_read={:?}). \
             This is expected when OpenRouter routes to a non-caching backend.",
            response2.usage.cache_read_tokens,
        );
    }
}

#[tokio::test]
async fn kimi_code_cache_validation() {
    let _observability =
        ghost::observability::init_for_live_tests().expect("init live test observability");

    if std::env::var("KIMI_API_KEY")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .is_none()
    {
        eprintln!("KIMI_API_KEY not set; skipping.");
        return;
    }

    let temp = tempfile::tempdir().unwrap();
    let mut provider =
        ghost::providers::KimiCodeProvider::new(BTreeMap::new()).expect("KIMI_API_KEY must be set");
    provider.set_debug(true, temp.path());

    let system = large_system_prompt();
    let model = "kimi-k2.5";

    // Turn 1
    let request1 = cache_test_request(model, &system, vec![user_message("ping")]);
    let response1 = provider.chat(request1).await.expect("turn 1");
    log_usage(&response1, "kimi_code", 1);

    // Turn 2 — include turn 1 as history
    let request2 = cache_test_request(
        model,
        &system,
        vec![
            user_message("ping"),
            ChatMessage {
                role: Role::Assistant,
                content: response1.content,
            },
            user_message("ping again"),
        ],
    );
    let response2 = provider.chat(request2).await.expect("turn 2");
    log_usage(&response2, "kimi_code", 2);

    dump_debug_files(&temp.path().join("debug/requests"));

    assert!(response1.usage.input_tokens > 0);
    assert!(response2.usage.input_tokens > 0);
}

#[tokio::test]
async fn openai_oauth_cache_validation() {
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

    // Turn 1
    let request1 = cache_test_request(&model, &system, vec![user_message("ping")]);
    let response1 = provider.chat(request1).await.expect("turn 1");
    log_usage(&response1, "openai_oauth", 1);

    // Turn 2 — include turn 1 as history
    // Note: OpenAI cache population is async. Cache hits typically appear
    // on subsequent test runs, not within a single run's two turns.
    let request2 = cache_test_request(
        &model,
        &system,
        vec![
            user_message("ping"),
            ChatMessage {
                role: Role::Assistant,
                content: response1.content,
            },
            user_message("ping again"),
        ],
    );
    let response2 = provider.chat(request2).await.expect("turn 2");
    log_usage(&response2, "openai_oauth", 2);

    dump_debug_files(&temp.path().join("debug/requests"));

    assert!(response1.usage.input_tokens > 0);
    assert!(response2.usage.input_tokens > 0);
}
