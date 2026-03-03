use std::time::Duration;

use crate::e2e::harness;

/// Continue the deep-research agent with a follow-up prompt. Loads from
/// step_04 so the workspace includes structured notes and references created
/// by reflection agents — the continued agent can leverage those via
/// knowledge_search. Since deep-research has no `on_resume` hook, the prompt
/// is appended as a new user message and the tool loop runs with full history.
#[tokio::test]
async fn printer_3d_step_05_continue_agent() {
    let loaded = harness::load_previous_step_or_fail(
        harness::SCENARIO_PRINTER_3D,
        harness::STEP_05,
        harness::STEP_04,
    )
    .await;
    let env = loaded.env;
    let prev = loaded.state;

    let session = prev.chat_session_id.clone();

    let agent_session = prev
        .agent_session_id
        .clone()
        .expect("step_04 must provide agent_session_id");

    let result = tokio::time::timeout(
        Duration::from_secs(600),
        env.agent_runner.resume(
            &agent_session,
            "The user wants a printer with strong multicolor capability, \
             with low waste. Update your research and recommendations \
             accordingly.",
            "deep-research",
        ),
    )
    .await
    .expect("TIMEOUT: agent did not complete in step_05 within 10 minutes")
    .expect("resume (continue) failed");
    let findings = result.findings;

    assert!(
        findings.len() > 200,
        "expected substantial findings in step_05, got {} chars",
        findings.len()
    );

    let findings_lower = findings.to_lowercase();
    assert!(
        findings_lower.contains("u1"),
        "expected Snapmaker U1 to appear in continued research findings. \
         Findings preview: {}",
        truncate_preview(&findings)
    );

    let mut state = harness::fresh_step_state(
        harness::SCENARIO_PRINTER_3D,
        harness::STEP_05,
        Some(harness::STEP_04),
        session,
    );
    state.agent_id = prev.agent_id;
    state.agent_session_id = Some(agent_session);
    state.final_response_preview = Some(truncate_preview(&findings));
    state.assertion_markers.insert(
        "continued_findings".to_string(),
        serde_json::json!(findings),
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
