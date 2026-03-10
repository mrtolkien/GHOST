use std::time::Duration;

use crate::stepwise::harness;

/// Step 02: User confirms project creation. GHOST should create the project
/// and set up initial tasks via `ghost project init` and `ghost project task create`.
#[tokio::test]
async fn project_create_step_02_confirm_and_create() {
    let harness::LoadedStep { env, state } = harness::load_previous_step_or_fail(
        harness::SCENARIO_PROJECT_CREATE,
        harness::STEP_PC_02,
        harness::STEP_PC_01,
    )
    .await;

    let session = &state.chat_session_id;
    let chat = env.chat();

    let (result, _metadata) = tokio::time::timeout(
        Duration::from_secs(120),
        chat.chat(
            session,
            "Yes, please create a project for this and set up the initial tasks.",
            None,
            None,
        ),
    )
    .await
    .expect("TIMEOUT: chat should respond within 120s")
    .expect("chat response failed in step_02");

    assert!(
        !result.message.trim().is_empty(),
        "expected a non-empty response after project creation"
    );

    let tool_calls = env.collect_tool_calls(session).await;

    // GHOST should have called run_shell_command (to run ghost project init)
    assert!(
        tool_calls.iter().any(|t| t == "run_shell_command"),
        "expected run_shell_command call for project creation, got: {tool_calls:?}"
    );

    // Verify project directory was created
    assert!(
        env.workspace_file_exists("projects"),
        "projects/ directory should exist"
    );

    // Find any project directory with an index.md
    let projects_dir = env.workspace_path().join("projects");
    let has_project = std::fs::read_dir(&projects_dir)
        .expect("read projects dir")
        .filter_map(|e| e.ok())
        .any(|entry| {
            entry.path().is_dir()
                && !entry.file_name().to_string_lossy().starts_with('.')
                && entry.path().join("index.md").exists()
        });
    assert!(has_project, "expected at least one project with index.md");

    let mut new_state = harness::fresh_step_state(
        harness::SCENARIO_PROJECT_CREATE,
        harness::STEP_PC_02,
        Some(harness::STEP_PC_01),
        session.clone(),
    );
    new_state
        .assertion_markers
        .insert("chat_tool_calls".to_string(), serde_json::json!(tool_calls));

    harness::save_step_snapshot(&env, &new_state).await;
}
