#![cfg(feature = "live-tests")]

mod common;

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
    let definition = env.load_agent("deep-research");

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
                  budget around $1000. Check specialist sites like all3dp.com \
                  and auroratechchannel.com, plus reddit discussions. I want \
                  specific model recommendations with prices, including any \
                  recently released models I might not know about.";
    let system_prompt = definition.render_system_prompt(prompt);

    // Run with 5-minute timeout
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(300),
        session_chat.chat_agent(
            "deep-research",
            &session.to_string(),
            prompt,
            system_prompt,
            definition.max_iterations,
        ),
    )
    .await;

    // Diagnostics (always, even on failure)
    env.log_session_json("agent", &session).await;

    let result = result
        .expect("TIMEOUT: agent did not complete within 5 minutes")
        .expect("agent chat_agent failed");

    env.log(format!(
        "findings ({} chars): {}",
        result.message.len(),
        result.message
    ));

    // --- Assertions ---

    // Non-trivial findings
    assert!(
        result.message.len() > 200,
        "expected substantial findings (>200 chars), got {} chars",
        result.message.len()
    );

    // Count web_fetch tool calls and collect URLs
    let messages = ghost::db::sessions::list_messages_by_session(&env.db, &session)
        .await
        .expect("list agent messages");

    let mut web_fetch_count = 0u32;
    let mut web_fetch_urls: Vec<String> = Vec::new();

    for msg in &messages {
        if let Some(ref calls) = msg.tool_calls {
            for call in calls {
                let name = call
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                if name == "web_fetch" {
                    web_fetch_count += 1;
                    if let Some(url) = call
                        .get("input")
                        .and_then(|v| v.get("url"))
                        .and_then(|v| v.as_str())
                    {
                        web_fetch_urls.push(url.to_string());
                    }
                }
            }
        }
    }

    env.log(format!("web_fetch calls: {web_fetch_count}"));
    env.log(format!("web_fetch urls: {web_fetch_urls:?}"));

    // Process quality: agent should fetch multiple pages
    assert!(
        web_fetch_count >= 5,
        "expected >= 5 web_fetch calls, got {web_fetch_count}"
    );

    // Domain specialist check
    let has_all3dp = web_fetch_urls.iter().any(|url| url.contains("all3dp.com"));
    assert!(has_all3dp, "expected at least one all3dp.com fetch");

    // Second domain specialist check (reddit favorite, best independant)
    let has_aurora = web_fetch_urls
        .iter()
        .any(|url| url.contains("auroratechchannel.com"));
    assert!(
        has_aurora,
        "expected at least one aurora tech channel fetch"
    );

    // Correct recommendation: the P2S should be in the list
    let findings_lower = result.message.to_lowercase();
    assert!(
        findings_lower.contains("p2s"),
        "expected 'P2S' in findings (case-insensitive)"
    );
}
