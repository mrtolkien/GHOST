use std::time::Duration;

use crate::stepwise::harness;

/// Step 02: The coding agent receives a message and edits code in the repo.
///
/// Pre-conditions (from step 01 snapshot):
/// - A coding session is active
/// - `code/test-repo/greeting.py` exists with `def greet(): return "hello"`
///
/// Assertions:
/// - The coding agent responds to a user message
/// - The coding agent calls file editing tools (file_write or file_edit)
/// - `greeting.py` is modified
#[tokio::test]
async fn coding_agent_step_02_coding_agent_edits() {
    let harness::LoadedStep { env, state } = harness::load_previous_step_or_fail(
        harness::SCENARIO_CODING_AGENT,
        harness::STEP_CA_02,
        harness::STEP_CA_01,
    )
    .await;

    // Retrieve coding session info from step 01
    let coding_session_id = state.assertion_markers["coding_chat_session_id"]
        .as_str()
        .expect("coding_chat_session_id should be a string");
    let relative_working_dir = state.assertion_markers["working_dir"]
        .as_str()
        .expect("working_dir should be a string");
    // Resolve the relative working_dir against the restored workspace
    let working_dir = env.workspace_path().join(relative_working_dir);

    // Verify the repo file exists in the restored snapshot
    assert!(
        working_dir.join("greeting.py").exists(),
        "greeting.py should exist in restored workspace at {}",
        working_dir.display()
    );

    let original =
        std::fs::read_to_string(working_dir.join("greeting.py")).expect("read greeting.py");
    assert!(
        original.contains("hello"),
        "original greeting.py should contain 'hello'"
    );

    // Build the coding agent system prompt
    let system_prompt = ghost::coding::prompt::build_coding_prompt(&env.config, &working_dir);

    // Create a SessionChat for the coding agent
    let chat = ghost::chat::SessionChat::from_config(env.db.clone(), env.config.clone())
        .expect("build session chat");

    // Send a coding request to the coding agent session
    let (result, _metadata) = tokio::time::timeout(
        Duration::from_secs(180),
        chat.chat_coding(
            coding_session_id,
            "Change the greet function to return \"hello world\" instead of \"hello\". \
             Edit the file directly.",
            &system_prompt,
            &working_dir,
            None,
            None,
        ),
    )
    .await
    .expect("TIMEOUT: coding agent should respond within 180s")
    .expect("coding agent chat failed in step_02");

    env.log_session_json("coding_agent", coding_session_id)
        .await;

    assert!(
        !result.message.trim().is_empty(),
        "coding agent should respond with a non-empty message"
    );

    // Verify the coding agent used file editing tools
    let messages = ghost::db::sessions::list_messages_by_session(&env.db, coding_session_id)
        .await
        .expect("list coding session messages");

    let edit_tools = ["file_write", "file_edit"];
    let has_file_edit = messages.iter().any(|msg| {
        msg.tool_calls_parsed()
            .map(|calls| {
                calls.iter().any(|c| {
                    let name = c.get("name").and_then(|v| v.as_str()).unwrap_or_default();
                    edit_tools.contains(&name)
                })
            })
            .unwrap_or(false)
    });
    assert!(
        has_file_edit,
        "coding agent should use file_write or file_edit to modify the code"
    );

    // Verify greeting.py was actually modified
    let modified = std::fs::read_to_string(working_dir.join("greeting.py"))
        .expect("read modified greeting.py");
    assert!(
        modified.contains("hello world"),
        "greeting.py should now contain 'hello world', got:\n{modified}"
    );

    // Collect tool calls for metrics
    let tool_calls: Vec<String> = messages
        .iter()
        .filter_map(|msg| msg.tool_calls_parsed())
        .flat_map(|calls| {
            calls
                .iter()
                .filter_map(|c| c.get("name").and_then(|v| v.as_str()).map(String::from))
                .collect::<Vec<_>>()
        })
        .collect();

    let mut new_state = harness::fresh_step_state(
        harness::SCENARIO_CODING_AGENT,
        harness::STEP_CA_02,
        Some(harness::STEP_CA_01),
        state.chat_session_id.clone(),
    );
    new_state.assertion_markers.insert(
        "coding_tool_calls".to_string(),
        serde_json::json!(tool_calls),
    );
    new_state
        .assertion_markers
        .insert("modified_content".to_string(), serde_json::json!(modified));

    harness::save_step_snapshot(&env, &new_state).await;
}
