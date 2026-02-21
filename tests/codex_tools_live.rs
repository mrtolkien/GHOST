#![cfg(feature = "live-tests")]

mod common;

/// Validates that gpt-5.3-codex via the Codex Responses API actually
/// calls tools when explicitly asked.
///
/// This is a smoke test: if it fails, tool calling is broken at the
/// provider level. If it passes, the agent's zero-tool-use is a
/// prompt/model behavior issue, not a wiring bug.
///
/// ```sh
/// cargo test --features live-tests codex_tool_calling_smoke -- --nocapture
/// ```
#[tokio::test]
async fn codex_tool_calling_smoke() {
    let env = common::live_test_database("codex_tool_smoke").await;
    let session = env.create_session().await;

    let chat = env.chat();
    let result = chat
        .chat(
            &session.to_string(),
            "Use the web_search tool to search for 'rust programming language'. \
             You MUST call web_search. After searching, tell me the first result title.",
        )
        .await
        .expect("chat failed");

    env.log_session_json("chat", &session).await;
    env.log(format!("response: {}", result.message));

    // Check that web_search was actually called
    let messages = ghost::db::sessions::list_messages_by_session(&env.db, &session)
        .await
        .expect("list messages");

    let web_search_count: u32 = messages
        .iter()
        .filter_map(|msg| msg.tool_calls.as_ref())
        .flat_map(|calls| calls.iter())
        .filter(|call| {
            call.get("name")
                .and_then(|v| v.as_str())
                .is_some_and(|n| n == "web_search")
        })
        .count() as u32;

    env.log(format!("web_search calls: {web_search_count}"));

    assert!(
        web_search_count >= 1,
        "expected at least 1 web_search call, got {web_search_count}"
    );
}
