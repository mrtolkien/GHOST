use std::time::Duration;

use crate::common;
use crate::stepwise::harness;

/// User asks about Ark Nova rules → GHOST finds the PDF rulebook online,
/// imports it via docling (page import with PDF fallback), and answers
/// the question using the imported reference.
///
/// Phase 1: Chat → GHOST reads skill, finds PDF URL, starts background import
/// Phase 2: Completion watcher triggers follow-up → GHOST searches refs, answers
#[tokio::test]
async fn ark_nova_step_01_pdf_rules_import() {
    let env = common::live_test_database("ark_nova_step_01").await;
    let session = env.create_session().await;
    let (chat, _watcher_handle) = env.chat_with_event_handler();

    // --- Phase 1: User asks about Ark Nova setup (5 min timeout) ---
    env.log("Phase 1: user asks about Ark Nova game rules");

    let (phase1_result, _metadata) = tokio::time::timeout(
        Duration::from_secs(300),
        chat.chat(
            &session,
            "How do you set up a game of Ark Nova? I need the full setup procedure \
             from the official rules. The rulebook is a PDF available online.",
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

    // Phase 1 assertions: GHOST should have started a background import
    let tool_calls = env.collect_tool_calls(&session).await;
    env.log(format!("Phase 1 tool calls: {tool_calls:?}"));

    assert!(
        tool_calls.iter().any(|name| name == "run_shell_command"),
        "Phase 1: GHOST should have called run_shell_command to import the PDF, \
         tool calls: {tool_calls:?}",
    );

    assert!(
        !phase1_result.message.trim().is_empty(),
        "Phase 1: response should be a non-empty acknowledgment"
    );

    let messages = ghost::db::sessions::list_messages_by_session(&env.db, &session)
        .await
        .expect("list messages");
    let phase1_message_count = messages.len();

    // --- Phase 2: Wait for completion watcher continuation (20 min) ---
    env.log("Phase 2: waiting for PDF import completion and continuation");

    let continuation = env
        .wait_for_continuation_response(&session, phase1_message_count, 1200)
        .await;

    let continuation_text = continuation
        .expect("Phase 2: completion watcher should have triggered a continuation response");

    env.log(format!(
        "Phase 2 response: {}",
        truncate_preview(&continuation_text)
    ));
    env.log_session_json_since("phase2_continuation", &session)
        .await;

    // Phase 2 assertions: continuation should mention setup concepts from the rules
    let response_lower = continuation_text.to_lowercase();
    let setup_terms = [
        "zoo map",
        "action card",
        "association",
        "conservation",
        "enclosure",
        "setup",
    ];
    assert!(
        setup_terms.iter().any(|term| response_lower.contains(term)),
        "Phase 2: continuation should mention Ark Nova setup concepts, got: {}",
        truncate_preview(&continuation_text),
    );

    // References should exist in the database
    let ref_count = ghost::db::knowledge::count_references(&env.db)
        .await
        .expect("count references");
    env.log(format!("reference count in DB: {ref_count}"));
    assert!(
        ref_count > 0,
        "GHOST should have imported the PDF rules as a reference"
    );

    // References should be on disk
    let refs = env.list_references();
    env.log(format!("reference files on disk: {refs:?}"));
    assert!(
        !refs.is_empty(),
        "Reference files should exist in the workspace"
    );

    // --- Save snapshot ---
    let state = harness::fresh_step_state(
        harness::SCENARIO_ARK_NOVA,
        harness::STEP_AN_01,
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
