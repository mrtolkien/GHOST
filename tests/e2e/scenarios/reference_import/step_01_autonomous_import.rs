use std::time::Duration;

use crate::common;
use crate::e2e::harness;

/// Two-phase autonomous test: user asks GHOST to import Dioxus docs, then asks
/// about hooks. GHOST uses background shell commands for the slow import.
///
/// Phase 1: Chat → GHOST reads skill, finds repo, starts background import
/// Phase 2: Follow-up chat → GHOST searches imported refs, answers about hooks
///
/// Note: we don't wait for `[shell-command completed]` because embedding 179
/// files can take 30+ minutes on slow GPUs. The reference *records* are in the
/// DB almost immediately; only the embeddings trail behind. The model can search
/// whatever's embedded so far (+ fall back to web search).
#[tokio::test]
async fn reference_import_step_01_autonomous_import() {
    let env = common::live_test_database("reference_import_step_01").await;
    let session = env.create_session().await;
    let chat = env.chat();

    // --- Phase 1: Trigger the import only (5 min timeout) ---
    env.log("Phase 1: triggering import");

    let (_phase1_result, _metadata) = tokio::time::timeout(
        Duration::from_secs(300),
        chat.chat(
            &session,
            "Import the Dioxus documentation into my knowledge base. \
             I'll ask questions about it once the import is done.",
            None,
        ),
    )
    .await
    .expect("TIMEOUT: Phase 1 should complete within 5 minutes")
    .expect("Phase 1 chat failed");

    env.log_session_json("phase1_chat", &session).await;

    // Phase 1 assertion: GHOST should have called run_shell_command to import.
    let tool_calls = env.collect_tool_calls(&session).await;
    env.log(format!("Phase 1 tool calls: {tool_calls:?}"));

    assert!(
        tool_calls.iter().any(|name| name == "run_shell_command"),
        "Phase 1: GHOST should have called run_shell_command to start the import, \
         tool calls: {tool_calls:?}",
    );

    // Verify background=true was used by checking for the background ack in
    // a tool result.
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

    // --- Phase 2: Follow-up chat to search and answer (5 min) ---
    env.log("Phase 2: follow-up chat about hooks");

    let (phase2_result, _metadata) = tokio::time::timeout(
        Duration::from_secs(300),
        chat.chat(
            &session,
            "Great, now explain how hooks work in Dioxus. Search the imported \
             references for relevant docs.",
            None,
        ),
    )
    .await
    .expect("TIMEOUT: Phase 2 should complete within 5 minutes")
    .expect("Phase 2 chat failed");

    env.log(format!(
        "Phase 2 response: {}",
        truncate_preview(&phase2_result.message)
    ));
    env.log_session_json_since("phase2_chat", &session).await;

    // Phase 2 assertion: response mentions Dioxus-relevant terms
    let response_lower = phase2_result.message.to_lowercase();
    let relevant_terms = ["hook", "signal", "use_signal", "use_effect", "dioxus"];
    assert!(
        relevant_terms
            .iter()
            .any(|term| response_lower.contains(term)),
        "Phase 2: response should mention Dioxus concepts, got: {}",
        truncate_preview(&phase2_result.message),
    );

    // References should exist in the database (git import stores content in
    // SQLite, not as files on disk).
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
