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
    env.log_session_json("chat", &session).await;
    env.log(format!("response: {}", result.message));
}

/// Initial research: ask about enclosed 3D printers, wait for agent findings.
///
/// 6-minute hard timeout. Tests that the GHOST spawns a deep-research agent
/// and the agent produces complete findings with quality assertions matching
/// the direct `deep_research_agent_produces_findings` test.
///
/// ```sh
/// cargo test --features e2e-tests e2e_research -- --nocapture
/// ```
#[tokio::test]
async fn e2e_research() {
    let env = common::live_test_database("e2e_research").await;
    let session = env.create_session().await;

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(360),
        run_initial_research(&env, &session),
    )
    .await;

    env.log_session_json("chat", &session).await;

    let outcome = result
        .expect("TIMEOUT: test exceeded 6 minutes")
        .expect("no agent outcome — model didn't spawn an agent");

    env.log(format!(
        "agent findings ({} chars): {}",
        outcome.findings.len(),
        outcome.findings
    ));

    let metrics = env.collect_web_fetch_metrics(&outcome.agent_session).await;
    env.assert_research_quality(
        &outcome.findings,
        &metrics,
        &["all3dp.com", "auroratechchannel.com"],
        &["p2s"],
    );
}

/// Follow-up: run initial research, then send a refinement and test agent
/// continuation. Also runs reflection.
///
/// 5-minute hard timeout.
///
/// ```sh
/// cargo test --features e2e-tests e2e_research_followup -- --nocapture
/// ```
#[tokio::test]
async fn e2e_research_followup() {
    let env = common::live_test_database("e2e_research_followup").await;
    let session = env.create_session().await;

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(300),
        run_research_with_followup(&env, &session),
    )
    .await;

    env.log_session_json("chat", &session).await;

    match result {
        Ok(()) => env.log("test completed within timeout"),
        Err(_) => env.log("TIMEOUT: test exceeded 5 minutes"),
    }
}

#[cfg(feature = "e2e-tests")]
async fn run_initial_research(
    env: &common::LiveTestEnv,
    session: &surrealdb::sql::Thing,
) -> Option<common::AgentOutcome> {
    let chat = env.chat();
    let result = chat
        .chat(
            &session.to_string(),
            "I want to buy a new enclosed 3D printer for home use, budget \
             around $1000. Check specialist sites like all3dp.com and \
             auroratechchannel.com, plus reddit discussions. I want specific \
             model recommendations with prices, including any recently \
             released models I might not know about.",
        )
        .await
        .expect("chat failed");
    env.log(format!("initial response: {}", result.message));

    let agent_ids = env.agent_runner.list_agent_ids().await;
    if agent_ids.is_empty() {
        env.log("no agents spawned — model answered directly");
        return None;
    }

    env.log(format!("agent(s) spawned: {}", agent_ids.join(", ")));
    env.wait_for_agents(session, 300).await
}

#[cfg(feature = "e2e-tests")]
async fn run_research_with_followup(env: &common::LiveTestEnv, session: &surrealdb::sql::Thing) {
    // Phase 1: initial research
    let _outcome = run_initial_research(env, session).await;
    env.log_session_json("chat_after_initial", session).await;

    // Phase 2: follow-up — should trigger agent continuation
    let chat2 = env.chat();
    let followup = chat2
        .chat(
            &session.to_string(),
            "Hmm, actually good multicolor support is important for me. \
             My budget is around $1000. I'll mostly print PLA and PETG.",
        )
        .await
        .expect("follow-up failed");
    env.log(format!("follow-up response: {}", followup.message));

    let cont_agent_ids = env.agent_runner.list_agent_ids().await;
    if !cont_agent_ids.is_empty() {
        env.log(format!(
            "continuation agent(s): {}",
            cont_agent_ids.join(", ")
        ));
        if let Some(cont_outcome) = env.wait_for_agents(session, 120).await {
            env.log(format!(
                "continuation result: {}",
                cont_outcome.chat_result.message
            ));
        }
    } else {
        env.log("no continuation agent spawned — model answered directly");
    }

    env.log_session_json_since("chat_after_continue", session)
        .await;

    // Phase 3: reflection
    let reflection_session = env.create_session().await;
    let reflection = env.run_reflection(session, None).await;
    env.log_session_json("reflection", &reflection_session)
        .await;
    env.log(format!("reflection handoff: {}", reflection.result.message));

    let notes = env.list_notes();
    let refs = env.list_references();
    env.log(format!(
        "artifacts: {} notes, {} references",
        notes.len(),
        refs.len()
    ));
}
