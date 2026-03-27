use std::time::Duration;

use crate::helpers::live_test_database;

/// Test: ask GHOST to create a daily recap agent from a chat message,
/// then verify the agent files and crontab entry were created, and
/// run the agent to confirm it produces findings.
#[tokio::test]
async fn test_cron_agent_creation() {
    let env = live_test_database("cron_agent_creation").await;
    let daemon = env.boot_daemon().await;

    let session_id = ghost::db::sessions::create_session(&daemon.db)
        .await
        .expect("create session");

    // Step 1: Ask GHOST to create a daily recap agent
    let timeout = Duration::from_secs(300);
    tokio::time::timeout(timeout, async {
        daemon
            .session_chat
            .chat(
                &session_id,
                "Please make me a recap of what's new from these websites every day at 7AM:\n\
                 - https://all3dp.com/3d-printing-news/\n\
                 - https://www.dpreview.com/feeds/news.xml\n\
                 - http://www.gsmarena.com/rss-news-reviews.php3",
                None,
                None,
            )
            .await
            .expect("chat failed");
    })
    .await
    .expect("TIMEOUT: agent creation exceeded 300s");

    daemon.settle().await.expect("settle after creation");

    // Step 2: Assert agent files were created
    let agents_dir = env.workspace_path().join("agents");
    let agent_dirs: Vec<_> = std::fs::read_dir(&agents_dir)
        .expect("read agents/")
        .filter_map(Result::ok)
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .filter(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            // Exclude known bundled agents/dirs
            name != "chat-reflection" && name != ".types"
        })
        .collect();

    assert!(
        !agent_dirs.is_empty(),
        "expected GHOST to create an agent directory under agents/"
    );

    let agent_dir = &agent_dirs[0];
    let agent_name = agent_dir.file_name().to_string_lossy().to_string();

    let agent_lua = agent_dir.path().join("agent.lua");
    assert!(agent_lua.exists(), "expected agents/{agent_name}/agent.lua");

    let prompt_md = agent_dir.path().join("prompt.md");
    assert!(prompt_md.exists(), "expected agents/{agent_name}/prompt.md");

    // Verify agent.lua references call_tool and the URLs
    let lua_content = std::fs::read_to_string(&agent_lua).expect("read agent.lua");
    assert!(
        lua_content.contains("call_tool"),
        "agent.lua should use ctx:call_tool or ctx:call_tools for pre-fetching"
    );
    assert!(
        lua_content.contains("web_fetch"),
        "agent.lua should call web_fetch tool"
    );

    // Step 3: Verify crontab.lua has the new entry
    let crontab_entries =
        ghost::agents::crontab::load_crontab(env.workspace_path()).expect("parse crontab.lua");
    let has_new_entry = crontab_entries.iter().any(|e| e.run == agent_name);
    assert!(
        has_new_entry,
        "crontab.lua should contain entry for '{agent_name}', found: {:?}",
        crontab_entries.iter().map(|e| &e.run).collect::<Vec<_>>()
    );

    // Step 4: Run the created agent
    env.log("running created agent...");
    let agent_result = daemon
        .agent_runner
        .run(
            &agent_name,
            "Execute the scheduled agent.",
            Some(&session_id),
        )
        .await
        .expect("agent run failed");

    let findings = &agent_result.findings;
    env.log(format!("agent findings length: {}", findings.len()));

    // Step 5: Assert findings contain content from at least one feed
    let findings_lower = findings.to_lowercase();
    let has_feed_content = findings_lower.contains("dpreview")
        || findings_lower.contains("gsmarena")
        || findings_lower.contains("3d print")
        || findings_lower.contains("phone")
        || findings_lower.contains("camera");
    assert!(
        has_feed_content,
        "agent findings should reference content from the RSS feeds, got: {}",
        &findings[..findings.len().min(500)]
    );

    assert!(
        findings.len() > 200,
        "agent findings should be a substantive recap, got {} chars",
        findings.len()
    );

    env.log_session_json("creation_chat", &session_id).await;

    daemon.shutdown().await;
}
