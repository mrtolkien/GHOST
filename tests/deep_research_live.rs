#![cfg(feature = "live-tests")]

mod common;

use ghost::db::fmt_id;

/// End-to-end test for the deep research agent.
///
/// Loads the agent definition from the temp workspace (repo-current),
/// builds a `SessionChat` with the agent's tools and model, runs the
/// agent tool loop with a 5-minute timeout, and asserts on output quality.
///
/// ```sh
/// cargo test --features live-tests deep_research_agent_produces_findings -- --nocapture
/// ```
#[tokio::test]
async fn deep_research_agent_produces_findings() {
    let env = common::live_test_database("deep_research").await;
    let definition = env.load_task("deep-research");

    // Build provider + tool manager from agent definition
    let provider = ghost::providers::provider_for_alias(&env.config, definition.model.as_deref())
        .expect("provider for agent model");
    let tool_manager = ghost::tools::ToolManager::for_agent(&definition.tools);
    let session_chat =
        ghost::chat::SessionChat::new(env.db.clone(), provider, tool_manager, env.config.clone())
            .with_max_tool_iterations(definition.max_iterations);

    // Create agent session + render system prompt
    let session = ghost::db::sessions::create_agent_session(&env.db)
        .await
        .expect("create agent session");
    let prompt = "Research the best enclosed 3D printers for home use in 2026, \
                  budget around $1000. I want specific model recommendations \
                  with prices, including any recently released models I might \
                  not know about.";
    let system_prompt = definition.render_system_prompt(prompt);

    // Run with 8-minute timeout
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(480),
        session_chat.chat_agent(&fmt_id(&session), prompt, system_prompt, &definition, None),
    )
    .await;

    // Diagnostics (always, even on failure)
    env.log_session_json("agent", &session).await;

    let (result, _metadata) = result
        .expect("TIMEOUT: agent did not complete within 8 minutes")
        .expect("agent chat_agent failed");

    env.log(format!(
        "findings ({} chars): {}",
        result.message.len(),
        result.message
    ));

    // --- Assertions (shared with e2e_research) ---
    let metrics = env.collect_web_fetch_metrics(&session).await;
    env.assert_research_quality(
        &result.message,
        &metrics,
        &["all3dp.com", "auroratechchannel.com"],
        &["p2s"],
    );
}
