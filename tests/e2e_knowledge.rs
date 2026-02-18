#![cfg(feature = "e2e-tests")]

mod common;

/// Smoke test: verify the model actually uses tools when asked directly.
///
/// Asks the model to call `run_shell_command("pwd")` and use `respond`.
/// If this fails, tools aren't reaching the model properly.
///
/// ```sh
/// cargo test --features e2e-tests e2e_tool_smoke -- --nocapture
/// ```
#[tokio::test]
async fn e2e_tool_smoke() {
    let env = common::live_test_database("e2e_tool_smoke").await;
    let session = env.create_session().await;

    let chat = env.chat();
    let result = chat
        .chat(
            &session.to_string(),
            "Use run_shell_command to run `pwd`, then use the respond tool to \
             tell me what directory you're in. You MUST use both tools.",
        )
        .await
        .expect("chat failed");
    env.log_session("tool_smoke", &session).await;
    env.log(format!("response: {}", result.message));
    env.log(format!("citations: {:?}", result.citations));
}

/// Full e2e: ask about enclosed 3D printers, wait for agent, then reflect.
///
/// The model may spawn a deep-research agent. If it does, we poll until
/// the agent finishes, inject findings into the session (like the daemon
/// watcher would), and trigger a follow-up turn before running reflection.
///
/// ```sh
/// cargo test --features e2e-tests e2e_3d_printers -- --nocapture
/// ```
#[tokio::test]
async fn e2e_3d_printers() {
    let env = common::live_test_database("e2e_3d_printers").await;
    let session = env.create_session().await;

    // Initial chat — may spawn a deep-research agent or answer directly
    let chat = env.chat();
    let result = chat
        .chat(
            &session.to_string(),
            "I want to buy a new 3d printer. Enclosed, for home use. What do you recommend?",
        )
        .await
        .expect("chat failed");
    env.log_session("initial_chat", &session).await;
    env.log(format!("initial response: {}", result.message));

    // Wait for background agents (5-minute timeout for deep research)
    let agent_ids = env.agent_runner.list_agent_ids().await;
    if !agent_ids.is_empty() {
        env.log(format!("agent(s) spawned: {}", agent_ids.join(", ")));

        if let Some(agent_result) = env.wait_for_agents(&session, 300).await {
            env.log_session("after_agent", &session).await;
            env.log(format!(
                "agent follow-up response: {}",
                agent_result.message
            ));
        }
    } else {
        env.log("no agents spawned — model answered directly");
    }

    // Reflection
    let reflection = env.run_reflection(&session, None).await;
    env.log_session("reflection", &session).await;
    env.log(format!("reflection handoff: {}", reflection.result.message));

    // Log workspace artifacts for review
    let notes = env.list_notes();
    let refs = env.list_references();
    env.log(format!(
        "artifacts: {} notes, {} references",
        notes.len(),
        refs.len()
    ));
}
