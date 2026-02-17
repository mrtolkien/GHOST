#![cfg(feature = "live-tests")]

mod common;

use ghost::chat::SessionChat;
use ghost::jobs::heartbeat::{is_heartbeat_continue, load_prompt};
use ghost::jobs::reflection::{DEFAULT_REFLECTION_PROMPT, clear_web_cache, filter_transcript};
use ghost::prompt::{JobPromptContext, PromptRenderer};
use ghost::tools::{ToolManager, ToolSet};
use ghost::web::scan_web_cache;

// ---------------------------------------------------------------------------
// Heartbeat e2e: HEARTBEAT_CONTINUE flow
// ---------------------------------------------------------------------------

/// Full heartbeat e2e: real provider, real tools, fresh temp DB.
///
/// Sends a trivial conversation, then runs a heartbeat job. The model should
/// decide there's nothing meaningful to say and return HEARTBEAT_CONTINUE.
#[tokio::test]
async fn heartbeat_returns_heartbeat_continue() {
    let env = common::live_test_database("heartbeat_continue").await;

    // Create a session with a trivial exchange — nothing to follow up on
    let session_id = ghost::db::sessions::create_session(&env.db)
        .await
        .expect("create session");
    ghost::db::sessions::create_message(&env.db, &session_id, "user", "Hey, what's up?")
        .await
        .expect("user msg");
    ghost::db::sessions::create_message(
        &env.db,
        &session_id,
        "assistant",
        "Not much! Just here if you need anything.",
    )
    .await
    .expect("assistant msg");

    // Run heartbeat via chat_job (same path as HeartbeatManager.run_heartbeat)
    let chat =
        SessionChat::from_config(env.db.clone(), env.config.clone()).expect("build session chat");
    let prompt = load_prompt(
        &env.config.workspace,
        "heartbeat.md",
        "# Heartbeat Check\n\n\
         You are running a heartbeat check. The OPERATOR has been idle.\n\n\
         If there's nothing meaningful to say, respond with exactly: \
         HEARTBEAT_CONTINUE",
    );

    let result = chat
        .chat_job("heartbeat", &session_id.to_string(), &prompt, ToolSet::Chat)
        .await
        .expect("heartbeat chat_job");

    env.log_session("heartbeat", &session_id).await;

    assert!(
        is_heartbeat_continue(&result.result.message),
        "Expected HEARTBEAT_CONTINUE, got: {:?}",
        result.result.message
    );

    // Verify job_log was created
    let logs = ghost::db::job_logs::list_job_logs(&env.db, Some("heartbeat"), 10)
        .await
        .expect("list job logs");
    assert!(!logs.is_empty(), "expected at least 1 heartbeat job_log");
    assert_eq!(logs[0].status, "ok");
}

// ---------------------------------------------------------------------------
// Reflection e2e: blog fetch → reflection classifies reference
// ---------------------------------------------------------------------------

/// Full e2e: ask the GHOST about a blog, it fetches and responds, then run
/// reflection which should move the `.web-cache` entry to references.
#[tokio::test]
async fn reflection_classifies_blog_reference() {
    let env = common::live_test_database("reflection_blog").await;

    // -- Step 1: Chat — ask about the blog ------------------------------------
    let session_id = ghost::db::sessions::create_session(&env.db)
        .await
        .expect("create session");

    let chat =
        SessionChat::from_config(env.db.clone(), env.config.clone()).expect("build session chat");
    let result = chat
        .chat(
            &session_id.to_string(),
            "What's the latest post on https://blog.tolki.dev/ ? \
             Use `ghost web fetch` to read it, then tell me the title and a one-line summary.",
        )
        .await
        .expect("chat response");

    env.log_session("chat", &session_id).await;

    assert!(
        !result.message.trim().is_empty(),
        "GHOST should have responded with blog summary"
    );

    // -- Step 2: Verify web cache was populated --------------------------------
    let web_cache_listing = scan_web_cache(&env.config.workspace).expect("scan web cache");
    assert!(
        web_cache_listing.is_some(),
        "Expected .web-cache/ to have entries after fetching blog. \
         The GHOST should have used `ghost web fetch` which auto-caches."
    );
    let web_cache_listing = web_cache_listing.unwrap();
    assert!(
        web_cache_listing.contains("blog.tolki.dev"),
        "web cache should contain blog.tolki.dev entry, got:\n{web_cache_listing}"
    );

    // -- Step 3: Build and run reflection --------------------------------------
    let messages = ghost::db::sessions::list_messages_by_session(&env.db, &session_id)
        .await
        .expect("list messages");
    let transcript = filter_transcript(&messages);

    let renderer = PromptRenderer::new(env.config.clone());
    let reflection_prompt_body = load_prompt(
        &env.config.workspace,
        "reflection.md",
        DEFAULT_REFLECTION_PROMPT,
    );
    let interpolated = renderer
        .render_job_prompt(
            "reflection",
            &JobPromptContext {
                prompt_body: reflection_prompt_body,
                previous_handoff: None,
                diary_today: None,
                recent_messages: Some(transcript),
                web_cache_files: Some(web_cache_listing),
            },
        )
        .expect("render reflection prompt");

    // Run reflection with reflection tools (includes reference_manage)
    let reflection_chat = SessionChat::new(
        env.db.clone(),
        ghost::providers::provider_for_alias(&env.config, None).expect("provider"),
        ToolManager::for_reflection(),
        env.config.clone(),
    );

    let temp_session = ghost::db::sessions::create_session(&env.db)
        .await
        .expect("create temp session");

    let reflection_result = reflection_chat
        .chat_job(
            "reflection",
            &temp_session.to_string(),
            &interpolated,
            ToolSet::Reflection,
        )
        .await
        .expect("reflection chat_job");

    env.log_session("reflection", &temp_session).await;

    assert!(
        !reflection_result.result.message.trim().is_empty(),
        "reflection should produce a handoff note"
    );

    // -- Step 4: Verify web cache was moved to references ---------------------
    let refs_dir = env.config.workspace.join("knowledge/references");
    assert!(
        find_file_containing(&refs_dir, "blog.tolki.dev"),
        "Expected a reference file containing 'blog.tolki.dev' under {}, \
         reflection should have used reference_manage to move the web cache entry",
        refs_dir.display()
    );

    // Web cache should be cleared after successful reflection
    clear_web_cache(&env.config.workspace).expect("clear web cache");
    let remaining = scan_web_cache(&env.config.workspace).expect("scan after clear");
    assert!(
        remaining.is_none(),
        "web cache should be empty after clearing"
    );
}

/// Recursively search for any file under `dir` whose content contains `needle`.
fn find_file_containing(dir: &std::path::Path, needle: &str) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if find_file_containing(&path, needle) {
                return true;
            }
        } else if path.is_file()
            && let Ok(content) = std::fs::read_to_string(&path)
            && content.contains(needle)
        {
            return true;
        }
    }
    false
}
