use std::time::{Duration, Instant};

use crate::e2e::harness;

#[tokio::test]
async fn printer_3d_step_02_run_agent_completion() {
    let loaded = harness::load_previous_step_or_fail(
        harness::SCENARIO_PRINTER_3D,
        harness::STEP_02,
        harness::STEP_01,
    )
    .await;
    let env = loaded.env;
    let prev = loaded.state;

    let session = prev.chat_session_id.clone();
    let agent_id = prev
        .agent_id
        .clone()
        .expect("step_01 must provide agent_id");

    env.agent_runner
        .continue_agent(&agent_id, "Continue and finish this research task.", None)
        .await
        .expect("continue_agent from step_01 state");

    let deadline = Instant::now() + Duration::from_secs(600);
    let findings = loop {
        if Instant::now() >= deadline {
            panic!("TIMEOUT: agent did not complete in step_02 within 10 minutes");
        }

        if let Some((status, _parent)) = env.agent_runner.take_completed(&agent_id).await {
            break status.findings;
        }

        tokio::time::sleep(Duration::from_secs(3)).await;
    }
    .expect("completed agent missing findings");
    assert!(
        findings.len() > 200,
        "expected substantial findings in step_02, got {} chars",
        findings.len()
    );

    let agent_session = prev
        .agent_session_id
        .clone()
        .expect("step_01 must provide agent_session_id");
    let metrics = env.collect_web_fetch_metrics(&agent_session).await;

    env.assert_research_quality(
        &findings,
        &metrics,
        &["all3dp.com", "auroratechchannel.com"],
        &["p2s"],
    );

    let mut state = harness::fresh_step_state(
        harness::SCENARIO_PRINTER_3D,
        harness::STEP_02,
        Some(harness::STEP_01),
        session,
    );
    state.agent_id = Some(agent_id);
    state.agent_session_id = Some(agent_session);
    state.final_response_preview = Some(truncate_preview(&findings));
    state
        .assertion_markers
        .insert("agent_findings".to_string(), serde_json::json!(findings));

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
