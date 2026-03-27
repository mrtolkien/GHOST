use std::time::Duration;

use crate::stepwise::harness;

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

    let agent_session = prev
        .agent_session_id
        .clone()
        .expect("step_01 must provide agent_session_id");

    // Resume agent synchronously. The agent now ends by calling the
    // `report_findings` terminal tool, which spawns a reflection agent
    // as a side effect (spawns are returned but not processed here).
    let result = tokio::time::timeout(
        Duration::from_secs(600),
        Box::pin(env.agent_runner.resume(
            &agent_session,
            "Continue and finish this research task.",
            "deep-research",
        )),
    )
    .await
    .expect("TIMEOUT: agent did not complete in step_02 within 10 minutes")
    .expect("resume failed");
    let findings = result.findings;

    assert!(
        findings.len() > 200,
        "expected substantial findings in step_02, got {} chars",
        findings.len()
    );

    let metrics = env.collect_web_fetch_metrics(&agent_session).await;

    env.assert_research_quality(
        &findings,
        &metrics,
        &["all3dp.com", "auroratechchannel.com"],
        &["p2s"],
    );

    // Extract the structured report data stored by the report_findings handler.
    // The handler calls ctx:set("report_data", json) using the agent slug as key.
    let report_data = ghost::db::agent_state::get_state(&env.db, "deep-research", "report_data")
        .await
        .expect("query agent_state")
        .expect("report_findings handler must store report_data in agent state");

    // Validate it's valid JSON with the expected fields
    let parsed: serde_json::Value =
        serde_json::from_str(&report_data).expect("report_data must be valid JSON");
    assert!(
        parsed.get("report").and_then(|v| v.as_str()).is_some(),
        "report_data must contain a 'report' field"
    );
    assert!(
        parsed.get("sources").and_then(|v| v.as_array()).is_some(),
        "report_data must contain a 'sources' array"
    );

    let mut state = harness::fresh_step_state(
        harness::SCENARIO_PRINTER_3D,
        harness::STEP_02,
        Some(harness::STEP_01),
        session,
    );
    state.agent_id = Some(agent_session.clone());
    state.agent_session_id = Some(agent_session);
    state.final_response_preview = Some(truncate_preview(&findings));
    state
        .assertion_markers
        .insert("agent_findings".to_string(), serde_json::json!(findings));
    state
        .assertion_markers
        .insert("report_data".to_string(), serde_json::json!(report_data));

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
