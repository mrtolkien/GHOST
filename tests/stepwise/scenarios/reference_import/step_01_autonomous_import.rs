use std::time::Duration;

use crate::common;
use crate::stepwise::harness;

/// Single-message autonomous test: user asks about Dioxus → GHOST imports docs
/// in the background → completion watcher auto-triggers continuation → GHOST
/// searches refs and answers.
///
/// Phase 1: Chat → GHOST reads skill, finds repo, starts background import, ends turn
/// Phase 2: Completion watcher triggers follow-up → GHOST searches imported refs, answers
#[tokio::test]
async fn reference_import_step_01_autonomous_import() {
    let env = common::live_test_database("reference_import_step_01").await;
    let session = env.create_session().await;
    let (chat, _watcher_handle) = env.chat_with_event_handler();

    // --- Phase 1: Single user message (5 min timeout) ---
    env.log("Phase 1: single user message about Dioxus");

    let (phase1_result, _metadata) = tokio::time::timeout(
        Duration::from_secs(300),
        chat.chat(
            &session,
            "I want to learn about Dioxus — what is it, and how do hooks work?",
            None,
            None,
        ),
    )
    .await
    .expect("TIMEOUT: Phase 1 should complete within 5 minutes")
    .expect("Phase 1 chat failed");

    env.log(format!(
        "Phase 1 response: {}",
        truncate_preview(&phase1_result.message)
    ));
    env.log_session_json("phase1_chat", &session).await;

    // Phase 1 assertions: GHOST should have called run_shell_command with background=true
    let tool_calls = env.collect_tool_calls(&session).await;
    env.log(format!("Phase 1 tool calls: {tool_calls:?}"));

    assert!(
        tool_calls.iter().any(|name| name == "run_shell_command"),
        "Phase 1: GHOST should have called run_shell_command to start the import, \
         tool calls: {tool_calls:?}",
    );

    let messages = ghost::db::sessions::list_messages_by_session(&env.db, &session)
        .await
        .expect("list messages");
    let has_bg_ack = messages.iter().any(|msg| {
        msg.tool_results_parsed()
            .map(|results| {
                results.iter().any(|r| {
                    r.get("content")
                        .and_then(|v| v.as_str())
                        .is_some_and(|c| c.contains("background"))
                })
            })
            .unwrap_or(false)
    });
    assert!(
        has_bg_ack,
        "Phase 1: GHOST should have used background=true for the import"
    );

    assert!(
        !phase1_result.message.trim().is_empty(),
        "Phase 1: response should be a non-empty acknowledgment"
    );

    let phase1_message_count = messages.len();

    // --- Phase 2: Wait for completion watcher to trigger continuation (40 min) ---
    env.log("Phase 2: waiting for autonomous continuation");

    let continuation = env
        .wait_for_continuation_response(&session, phase1_message_count, 2400)
        .await;

    let continuation_text = continuation
        .expect("Phase 2: completion watcher should have triggered a continuation response");

    env.log(format!(
        "Phase 2 response: {}",
        truncate_preview(&continuation_text)
    ));
    env.log_session_json_since("phase2_continuation", &session)
        .await;

    // Phase 2 assertions: continuation mentions Dioxus concepts
    let response_lower = continuation_text.to_lowercase();
    let relevant_terms = ["hook", "signal", "use_signal", "use_effect", "dioxus"];
    assert!(
        relevant_terms
            .iter()
            .any(|term| response_lower.contains(term)),
        "Phase 2: continuation should mention Dioxus concepts, got: {}",
        truncate_preview(&continuation_text),
    );

    // References should exist in the database
    let ref_count = ghost::db::knowledge::count_references(&env.db)
        .await
        .expect("count references");
    env.log(format!("reference count in DB: {ref_count}"));
    assert!(
        ref_count > 0,
        "GHOST should have imported references into the database"
    );

    // --- Save snapshot ---
    let state = harness::fresh_step_state(
        harness::SCENARIO_REFERENCE_IMPORT,
        harness::STEP_RI_01,
        None,
        session,
    );
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
