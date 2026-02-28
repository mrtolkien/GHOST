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
    let (result, _metadata) = chat
        .chat(
            &session,
            "Use run_shell_command to run `pwd`, then use the respond tool to \
             tell me what directory you're in. You MUST use both tools.",
            None,
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

    // --- Reflection phase (3 min timeout each, sequential) ---

    // Agent session reflection: has full research transcript + web fetches
    let (_agent_reflection, _agent_meta) = tokio::time::timeout(
        std::time::Duration::from_secs(180),
        env.run_reflection(&outcome.agent_session, None, "reflection"),
    )
    .await
    .expect("TIMEOUT: agent reflection did not complete within 3 minutes");

    env.log_session_json_since("agent_reflection", &outcome.agent_session)
        .await;

    // Chat session reflection: user question + injected findings summary + diary
    let (_chat_reflection, _chat_meta) = tokio::time::timeout(
        std::time::Duration::from_secs(180),
        env.run_reflection(&session, None, "chat-reflection"),
    )
    .await
    .expect("TIMEOUT: chat reflection did not complete within 3 minutes");

    env.log_session_json_since("chat_reflection", &session)
        .await;

    // Product note for P2S
    env.assert_notes_contain_any(&["P2S", "p2s"], "Bambu Lab P2S");

    // Source quality note
    env.assert_notes_contain_any(
        &["all3dp", "All3DP", "aurora", "tomshardware"],
        "source quality",
    );

    // References should exist (curated from web cache)
    let refs = env.list_references();
    assert!(!refs.is_empty(), "expected at least one curated reference");

    // Diary entry from chat reflection
    let diary = env.assert_diary_exists();
    env.log(format!("diary ({} chars): {diary}", diary.len()));
}

/// Complex research question should trigger the deep-research agent.
///
/// Verifies the escalation ladder: the GHOST checks knowledge first, reads
/// the deep-research skill, then spawns the agent. We don't wait for the
/// agent to complete — we only care that the model decided to spawn it.
///
/// The chat may time out while the model polls the agent for completion;
/// that's fine — we check the session messages regardless.
///
/// ```sh
/// cargo test --features e2e-tests e2e_complex_query_spawns_agent -- --nocapture
/// ```
#[tokio::test]
async fn e2e_complex_query_spawns_agent() {
    let env = common::live_test_database("e2e_complex_query_spawns_agent").await;
    let session = env.create_session().await;

    let chat = env.chat();
    // The model will likely spawn the agent then poll for completion,
    // so the chat may not return within the timeout. That's expected.
    let _result = tokio::time::timeout(
        std::time::Duration::from_secs(120),
        chat.chat(
            &session,
            "I want to buy a corexy 3d printer to replace my Bambulab A1. \
             Horizontal desk space is a premium in my workspace, but I'd like \
             a tool changer to be able to easily print in PLA with PETG \
             supports, and generally for good multicolor. What do you recommend?",
            None,
        ),
    )
    .await;

    // Stop any spawned agents immediately (save API credits)
    let stopped = env.stop_all_agents().await;
    env.log(format!("stopped {stopped} agent(s)"));

    env.log_session_json("chat", &session).await;

    // Collect tool calls to verify the escalation ladder
    let tool_calls = env.collect_tool_calls(&session).await;
    env.log(format!("tool calls: {tool_calls:?}"));

    // 1. Should have checked knowledge first
    assert!(
        tool_calls.iter().any(|t| t == "knowledge_search"),
        "expected knowledge_search before web tools, got: {tool_calls:?}"
    );

    // 2. Should have spawned the deep-research agent
    assert!(
        tool_calls.iter().any(|t| t == "agent_control"),
        "expected agent_control (deep-research spawn), got: {tool_calls:?}"
    );

    // 3. knowledge_search should come before agent_control
    let ks_pos = tool_calls.iter().position(|t| t == "knowledge_search");
    let ac_pos = tool_calls.iter().position(|t| t == "agent_control");
    if let (Some(ks), Some(ac)) = (ks_pos, ac_pos) {
        assert!(
            ks < ac,
            "expected knowledge_search (pos {ks}) before agent_control (pos {ac})"
        );
    }
}

#[cfg(feature = "e2e-tests")]
async fn run_initial_research(
    env: &common::LiveTestEnv,
    session: &str,
) -> Option<common::AgentOutcome> {
    let chat = env.chat();
    let (result, _metadata) = chat
        .chat(
            session,
            "I want to buy a new enclosed 3D printer for home use, around $1000. What do you recommend?",
            None,
        )
        .await
        .expect("chat failed");
    env.log(format!("initial response: {}", result.message));

    let agent_ids = env.task_runner.list_task_ids().await;
    if agent_ids.is_empty() {
        env.log("no agents spawned — model answered directly");
        return None;
    }

    env.log(format!("agent(s) spawned: {}", agent_ids.join(", ")));
    env.wait_for_agents(session, 600).await
}
