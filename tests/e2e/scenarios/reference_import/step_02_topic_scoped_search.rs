use std::time::Duration;

use crate::e2e::harness;

/// Step 02: Send a follow-up that should be answered from imported refs.
///
/// Loads step 01 snapshot (which has Dioxus tutorial docs imported).
/// Sends a question that should be answerable purely from the local
/// references, verifying topic-scoped search quality.
#[tokio::test]
async fn reference_import_step_02_topic_scoped_search() {
    let loaded = harness::load_previous_step_or_fail(
        harness::SCENARIO_REFERENCE_IMPORT,
        harness::STEP_RI_02,
        harness::STEP_RI_01,
    )
    .await;
    let env = loaded.env;
    let prev = loaded.state;

    // Use a fresh session (we want an independent question)
    let session = env.create_session().await;
    let chat = env.chat();
    let (result, _metadata) = tokio::time::timeout(
        Duration::from_secs(120),
        chat.chat(
            &session,
            "Show me an example of using use_signal in Dioxus",
            None,
        ),
    )
    .await
    .expect("TIMEOUT: chat should respond")
    .expect("chat failed");

    assert!(
        !result.message.trim().is_empty(),
        "GHOST should return a non-empty response"
    );

    // Verify knowledge_search was called
    let tool_calls = env.collect_tool_calls(&session).await;
    assert!(
        tool_calls.iter().any(|t| t == "knowledge_search"),
        "GHOST should use knowledge_search, got: {tool_calls:?}"
    );

    // GHOST should NOT need to fetch from the web — the answer is in local refs
    assert!(
        !tool_calls
            .iter()
            .any(|t| t == "web_search" || t == "web_fetch"),
        "GHOST should use local references, not web search. Tools used: {tool_calls:?}"
    );

    let mut state = harness::fresh_step_state(
        harness::SCENARIO_REFERENCE_IMPORT,
        harness::STEP_RI_02,
        Some(harness::STEP_RI_01),
        session,
    );
    state.assertion_markers.insert(
        "references_created".to_string(),
        prev.assertion_markers
            .get("references_created")
            .cloned()
            .unwrap_or_default(),
    );
    state.assertion_markers.insert(
        "topic_id".to_string(),
        prev.assertion_markers
            .get("topic_id")
            .cloned()
            .unwrap_or_default(),
    );
    state.final_response_preview = Some(truncate_preview(&result.message));

    harness::save_step_snapshot(&env, &state).await;
}

fn truncate_preview(s: &str) -> String {
    let max = 300usize;
    if s.len() <= max {
        s.to_string()
    } else {
        let boundary = s.floor_char_boundary(max);
        format!("{}...", &s[..boundary])
    }
}
