use std::time::Duration;

use crate::common;
use crate::e2e::harness;

/// Step 01: User asks about building a keyboard with ergogen.
/// GHOST should read the project-manager skill, respond with a plan, and ask
/// before creating a project (should NOT create it yet).
#[tokio::test]
async fn project_create_step_01_propose_project() {
    let env = common::live_test_database("project_create_step_01").await;
    let session = env.create_session().await;

    let chat = env.chat();
    let (result, _metadata) = tokio::time::timeout(
        Duration::from_secs(120),
        chat.chat(
            &session,
            "I want to build a keyboard with ergogen, from scratch. \
             Tell me how to do this.",
            None,
        ),
    )
    .await
    .expect("TIMEOUT: chat should respond within 120s")
    .expect("chat response failed in step_01");

    assert!(
        !result.message.trim().is_empty(),
        "expected a non-empty text response"
    );

    let tool_calls = env.collect_tool_calls(&session).await;

    // GHOST should read the project-manager skill file
    assert!(
        tool_calls.iter().any(|t| t == "read_file"),
        "expected read_file call (skill reading) in step 01, got: {tool_calls:?}"
    );

    // GHOST should NOT have created a project yet — it should ask first
    let messages = ghost::db::sessions::list_messages_by_session(&env.db, &session)
        .await
        .expect("list messages");
    let has_project_init = messages.iter().any(|msg| {
        msg.tool_calls_parsed()
            .map(|calls| {
                calls.iter().any(|c| {
                    let name = c.get("name").and_then(|v| v.as_str()).unwrap_or_default();
                    let input = c
                        .get("input")
                        .and_then(|v| v.as_object())
                        .cloned()
                        .unwrap_or_default();
                    name == "run_shell_command"
                        && input
                            .get("command")
                            .and_then(|v| v.as_str())
                            .is_some_and(|cmd| cmd.contains("project init"))
                })
            })
            .unwrap_or(false)
    });
    assert!(
        !has_project_init,
        "GHOST should NOT create a project in step 01 — it should ask first"
    );

    let mut state = harness::fresh_step_state(
        harness::SCENARIO_PROJECT_CREATE,
        harness::STEP_PC_01,
        None,
        session,
    );
    state
        .assertion_markers
        .insert("chat_tool_calls".to_string(), serde_json::json!(tool_calls));

    harness::save_step_snapshot(&env, &state).await;
}
