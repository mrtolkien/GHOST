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

/// Full e2e: ask about enclosed 3D printers, then run reflection.
///
/// This is a manual review script — run it, then inspect the output in
/// `e2e-output/<timestamp>_e2e_3d_printers/` for the diagnostic log and
/// workspace snapshot (notes, references, diary created by reflection).
///
/// ```sh
/// cargo test --features e2e-tests e2e_3d_printers -- --nocapture
/// ```
#[tokio::test]
async fn e2e_3d_printers() {
    let env = common::live_test_database("e2e_3d_printers").await;
    let session = env.create_session().await;

    // Chat
    let chat = env.chat();
    let result = chat
        .chat(
            &session.to_string(),
            "I want to buy a new 3d printer. Enclosed, for home use. What do you recommend?",
        )
        .await
        .expect("chat failed");
    env.log_session("chat", &session).await;
    env.log(format!("chat response: {}", result.message));

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
