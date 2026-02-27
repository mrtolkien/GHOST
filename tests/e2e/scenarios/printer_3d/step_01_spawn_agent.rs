use std::time::Duration;

use crate::e2e::harness;

#[tokio::test]
async fn printer_3d_step_01_spawn_agent() {
    let env = common::live_test_database("printer_3d_step_01").await;
    let session = env.create_session().await;

    let chat = env.chat();
    let _ = tokio::time::timeout(
        Duration::from_secs(90),
        chat.chat(
            &session,
            "I want to buy a new enclosed 3D printer for home use around $1000 in 2026. \
             Please do deep research with concrete model recommendations, pricing, and sources.",
            None,
        ),
    )
    .await;

    let tool_calls = env.collect_tool_calls(&session).await;
    assert!(
        tool_calls.iter().any(|t| t == "agent_control"),
        "expected agent_control call in step 01, got: {tool_calls:?}"
    );

    let agent_ids = env.task_runner.list_task_ids().await;
    assert!(
        !agent_ids.is_empty(),
        "expected at least one spawned agent in step 01"
    );
    let agent_id = agent_ids[0].clone();
    let agent_session_id = agent_id
        .split_once(':')
        .map(|(_, id)| id.to_string())
        .unwrap_or_else(|| agent_id.clone());

    let mut state = harness::fresh_step_state(
        harness::SCENARIO_PRINTER_3D,
        harness::STEP_01,
        None,
        session,
    );
    state.agent_id = Some(agent_id);
    state.agent_session_id = Some(agent_session_id);
    state
        .assertion_markers
        .insert("chat_tool_calls".to_string(), serde_json::json!(tool_calls));

    harness::save_step_snapshot(&env, &state).await;
}

use crate::common;
