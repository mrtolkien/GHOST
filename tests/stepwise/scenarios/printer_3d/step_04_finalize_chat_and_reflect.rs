use std::time::Duration;

use crate::stepwise::harness;

#[tokio::test]
async fn printer_3d_step_04_finalize_chat_and_reflect() {
    let loaded = harness::load_previous_step_or_fail(
        harness::SCENARIO_PRINTER_3D,
        harness::STEP_04,
        harness::STEP_03,
    )
    .await;
    let env = loaded.env;
    let prev = loaded.state;

    let chat_session = prev.chat_session_id.clone();
    let findings = prev
        .assertion_markers
        .get("agent_findings")
        .and_then(|v| v.as_str())
        .expect("step_03 state must include agent_findings from step_02");

    let system_msg = format!("[agent:deep-research completed]\n\n{}", findings,);
    ghost::db::sessions::create_message(&env.db, &chat_session, "system", &system_msg)
        .await
        .expect("inject agent findings into chat session");

    let chat = env.chat();
    let (result, _metadata) = tokio::time::timeout(
        Duration::from_secs(180),
        chat.chat(
            &chat_session,
            "[system] Research agent completed.",
            None,
            None,
        ),
    )
    .await
    .expect("TIMEOUT: final chat response did not complete in step_04")
    .expect("chat response failed in step_04");

    assert!(
        !result.message.trim().is_empty(),
        "expected non-empty final response in step_04"
    );

    let (_chat_reflection, _chat_reflection_meta) = tokio::time::timeout(
        Duration::from_secs(180),
        env.run_reflection(&chat_session, "chat-reflection"),
    )
    .await
    .expect("TIMEOUT: chat reflection did not complete in step_04");

    let diary = env.assert_diary_exists();
    assert!(
        !diary.trim().is_empty(),
        "expected non-empty diary after step_04 chat reflection"
    );

    let mut state = harness::fresh_step_state(
        harness::SCENARIO_PRINTER_3D,
        harness::STEP_04,
        Some(harness::STEP_03),
        chat_session,
    );
    state.agent_id = prev.agent_id;
    state.agent_session_id = prev.agent_session_id;
    state.final_response_preview = Some(truncate_preview(&result.message));
    state.assertion_markers.insert(
        "final_response".to_string(),
        serde_json::json!(result.message),
    );

    harness::save_step_snapshot(&env, &state).await;
}

fn truncate_preview(s: &str) -> String {
    let max = 300usize;
    if s.len() <= max {
        s.to_string()
    } else {
        let boundary = s.floor_char_boundary(max);
        format!("{}…", &s[..boundary])
    }
}
