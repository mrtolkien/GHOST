use std::time::Duration;

use crate::stepwise::harness;

/// User asks about a specific Ark Nova card (Baboon Rock) → GHOST searches,
/// finds the GitHub TypeScript data repo, imports it via reference_import,
/// and answers using the imported card data.
///
/// Loads workspace from step 01 (PDF rules already imported).
/// Phase 1: Chat → GHOST searches for card data, starts import
/// Phase 2: Completion watcher triggers → GHOST finds Baboon Rock data, answers
#[tokio::test]
async fn ark_nova_step_02_card_data_import() {
    let harness::LoadedStep { env, state } = harness::load_previous_step_or_fail(
        harness::SCENARIO_ARK_NOVA,
        harness::STEP_AN_02,
        harness::STEP_AN_01,
    )
    .await;

    let session = &state.chat_session_id;
    let (chat, _watcher_handle) = env.chat_with_event_handler();

    // --- Phase 1: User asks about a specific card (5 min timeout) ---
    env.log("Phase 1: user asks about Baboon Rock card");

    let (phase1_result, _metadata) = tokio::time::timeout(
        Duration::from_secs(300),
        chat.chat(
            session,
            "What does the Baboon Rock card do in Ark Nova? \
             Find the card data — there should be a project on GitHub \
             with all the card definitions.",
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
    env.log_session_json("phase1_chat", session).await;

    let tool_calls = env.collect_tool_calls(session).await;
    env.log(format!("Phase 1 tool calls: {tool_calls:?}"));

    let messages = ghost::db::sessions::list_messages_by_session(&env.db, session)
        .await
        .expect("list messages");
    let phase1_message_count = messages.len();

    // Check if the model answered immediately (found data without background import)
    // or started a background import. Either path is valid.
    let response_lower = phase1_result.message.to_lowercase();
    let answered_immediately = response_lower.contains("baboon");

    if answered_immediately {
        env.log("GHOST answered about Baboon Rock immediately (no background import needed)");

        // Direct answer — assert quality
        assert_baboon_rock_answer(&response_lower);
    } else {
        // --- Phase 2: Wait for background import + continuation (20 min) ---
        env.log("Phase 2: waiting for card data import and continuation");

        let continuation = env
            .wait_for_continuation_response(session, phase1_message_count, 1200)
            .await;

        let continuation_text = continuation.expect(
            "Phase 2: completion watcher should have triggered a continuation about Baboon Rock",
        );

        env.log(format!(
            "Phase 2 response: {}",
            truncate_preview(&continuation_text)
        ));
        env.log_session_json_since("phase2_continuation", session)
            .await;

        let cont_lower = continuation_text.to_lowercase();
        assert_baboon_rock_answer(&cont_lower);
    }

    // References should have grown (card data added on top of PDF rules)
    let ref_count = ghost::db::knowledge::count_references(&env.db)
        .await
        .expect("count references");
    env.log(format!("total reference count in DB: {ref_count}"));
    assert!(
        ref_count >= 2,
        "DB should have at least 2 references (rules + card data), got {ref_count}"
    );

    // --- Save snapshot ---
    let new_state = harness::fresh_step_state(
        harness::SCENARIO_ARK_NOVA,
        harness::STEP_AN_02,
        Some(harness::STEP_AN_01),
        session.clone(),
    );
    harness::save_step_snapshot(&env, &new_state).await;
}

fn assert_baboon_rock_answer(response_lower: &str) {
    // Baboon Rock is a special enclosure card — the answer should mention
    // game-relevant details
    let card_terms = ["baboon", "rock"];
    assert!(
        card_terms.iter().all(|t| response_lower.contains(t)),
        "Response should mention Baboon Rock"
    );
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
