use std::time::Duration;

use crate::stepwise::harness;

#[tokio::test]
async fn printer_3d_step_03_reflect_agent() {
    let loaded = harness::load_previous_step_or_fail(
        harness::SCENARIO_PRINTER_3D,
        harness::STEP_03,
        harness::STEP_02,
    )
    .await;
    let env = loaded.env;
    let prev = loaded.state;

    let agent_session = prev
        .agent_session_id
        .clone()
        .expect("step_02 must provide agent_session_id");

    // Extract the structured report data saved by step_02.
    let report_data = prev
        .assertion_markers
        .get("report_data")
        .and_then(|v| v.as_str())
        .expect("step_02 must provide report_data in assertion_markers");

    // Run the structured reflection agent — it receives only the report
    // data (report, sources, secondary_info, negative_info), NOT the full
    // research conversation. This tests whether structured output alone
    // is sufficient for high-quality note extraction.
    let (findings, metadata) = tokio::time::timeout(
        Duration::from_secs(300),
        env.run_structured_reflection(&agent_session, report_data),
    )
    .await
    .expect("TIMEOUT: structured reflection did not complete in step_03");

    assert!(
        !findings.trim().is_empty(),
        "expected non-empty reflection findings in step_03"
    );

    let refs = env.list_references();
    assert!(
        !refs.is_empty(),
        "expected curated references after agent reflection in step_03"
    );

    let mut state = harness::fresh_step_state(
        harness::SCENARIO_PRINTER_3D,
        harness::STEP_03,
        Some(harness::STEP_02),
        prev.chat_session_id,
    );
    state.agent_id = prev.agent_id;
    state.agent_session_id = Some(agent_session);
    state.final_response_preview = Some(truncate_preview(&findings));
    state.assertion_markers.insert(
        "agent_reflection_handoff".to_string(),
        serde_json::json!(findings),
    );
    if let Some(agent_findings) = prev.assertion_markers.get("agent_findings") {
        state
            .assertion_markers
            .insert("agent_findings".to_string(), agent_findings.clone());
    }

    harness::save_step_snapshot_with_metadata(&env, &state, Some(&metadata)).await;
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
