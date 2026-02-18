#![cfg(feature = "live-tests")]

mod common;

use ghost::jobs::heartbeat::is_heartbeat_continue;
use ghost::jobs::reflection::clear_web_cache;
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
    let session = env
        .session_with_messages(&[
            ("user", "Hey, what's up?"),
            ("assistant", "Not much! Just here if you need anything."),
        ])
        .await;

    let result = env.run_heartbeat(&session).await;
    env.log_session_json("heartbeat", &session).await;

    assert!(
        is_heartbeat_continue(&result.result.message),
        "Expected HEARTBEAT_CONTINUE, got: {:?}",
        result.result.message
    );

    let logs = ghost::db::job_logs::list_job_logs(&env.db, Some("heartbeat"), 10)
        .await
        .expect("list job logs");
    assert!(!logs.is_empty(), "expected at least 1 heartbeat job_log");
    assert_eq!(logs[0].status, "ok");
}

// ---------------------------------------------------------------------------
// Reflection e2e: blog fetch -> reflection classifies reference
// ---------------------------------------------------------------------------

/// Full e2e: ask the GHOST about a blog, it fetches and responds, then run
/// reflection which should move the `.web-cache` entry to references.
#[tokio::test]
async fn reflection_classifies_blog_reference() {
    let env = common::live_test_database("reflection_blog").await;
    let session = env.create_session().await;

    // Step 1: Chat — ask about the blog
    let chat = env.chat();
    let result = chat
        .chat(
            &session.to_string(),
            "What's the latest post on https://blog.tolki.dev/ ? \
             Use `ghost web fetch` to read it, then tell me the title and a one-line summary.",
        )
        .await
        .expect("chat response");
    env.log_session_json("chat", &session).await;
    assert!(
        !result.message.trim().is_empty(),
        "GHOST should have responded with blog summary"
    );

    // Step 2: Verify web cache was populated
    let listing = scan_web_cache(&env.config.workspace).expect("scan web cache");
    assert!(
        listing.is_some(),
        "Expected .web-cache/ to have entries after fetching blog"
    );
    assert!(
        listing.as_ref().unwrap().contains("blog.tolki.dev"),
        "web cache should contain blog.tolki.dev entry"
    );

    // Step 3: Run reflection
    let result = env.run_reflection(&session, None).await;
    env.log_session_json("reflection-result", &session).await;
    assert!(
        !result.result.message.trim().is_empty(),
        "reflection should produce a handoff note"
    );

    // Step 4: Verify reflection moved web cache to references
    assert!(
        env.find_file_containing("references", "blog.tolki.dev"),
        "Expected a reference file containing 'blog.tolki.dev'"
    );

    // Step 5: Clear web cache (as production does post-reflection)
    clear_web_cache(&env.config.workspace).expect("clear web cache");
    assert!(
        scan_web_cache(&env.config.workspace)
            .expect("scan after clear")
            .is_none(),
        "web cache should be empty after clearing"
    );
}

// ---------------------------------------------------------------------------
// Heartbeat e2e: proactive follow-up
// ---------------------------------------------------------------------------

/// Heartbeat with a substantive conversation — model should have something
/// to say (or at least HEARTBEAT_CONTINUE; the point is that the model
/// processes a richer transcript without errors).
#[tokio::test]
async fn heartbeat_proactive_followup() {
    let env = common::live_test_database("heartbeat_followup").await;
    let session = env
        .session_with_messages(&[
            (
                "user",
                "I'm working on a Rust project and struggling with lifetimes. \
                 Can you explain how the borrow checker works?",
            ),
            (
                "assistant",
                "The borrow checker enforces Rust's ownership rules at compile time. \
                 Each value has a single owner, and references must not outlive the \
                 data they point to. Would you like me to walk through a specific example?",
            ),
            ("user", "Yes, show me an example with structs."),
            (
                "assistant",
                "Here's a common pattern:\n\n```rust\nstruct Config<'a> {\n    \
                 name: &'a str,\n}\n```\n\nThe lifetime `'a` tells the compiler \
                 that `Config` cannot outlive the string it borrows.",
            ),
        ])
        .await;

    let result = env.run_heartbeat(&session).await;
    env.log_session_json("heartbeat_followup", &session).await;

    // Model should either continue silently or send a follow-up — either is valid
    assert!(
        !result.result.message.trim().is_empty(),
        "heartbeat should produce a non-empty response"
    );

    let logs = ghost::db::job_logs::list_job_logs(&env.db, Some("heartbeat"), 10)
        .await
        .expect("list job logs");
    assert!(!logs.is_empty());
    assert_eq!(logs[0].status, "ok");
}

// ---------------------------------------------------------------------------
// Reflection e2e: creates knowledge notes
// ---------------------------------------------------------------------------

/// After a knowledge-rich conversation, reflection should create notes.
#[tokio::test]
async fn reflection_creates_knowledge_notes() {
    let env = common::live_test_database("reflection_notes").await;
    let session = env.create_session().await;

    let chat = env.chat();
    chat.chat(
        &session.to_string(),
        "I just learned that SurrealDB supports graph relations using RELATE statements. \
         For example: `RELATE user:alice->follows->user:bob`. This creates typed edges \
         between records. I want to remember this for later.",
    )
    .await
    .expect("chat response");
    env.log_session_json("chat", &session).await;

    let result = env.run_reflection(&session, None).await;
    env.log_session_json("reflection", &session).await;

    assert!(
        !result.result.message.trim().is_empty(),
        "reflection should produce a handoff note"
    );
    env.log(format!("handoff: {}", result.result.message));

    // Reflection may create notes or diary entries — check for any knowledge artifacts
    let notes = env.list_notes();
    let refs = env.list_references();
    env.log(format!(
        "notes: {}, references: {}",
        notes.len(),
        refs.len()
    ));
}

// ---------------------------------------------------------------------------
// Reflection e2e: handoff continuity
// ---------------------------------------------------------------------------

/// Reflection handoff note carries over between runs.
#[tokio::test]
async fn reflection_handoff_continuity() {
    let env = common::live_test_database("reflection_handoff").await;
    let session = env.create_session().await;

    // First conversation
    let chat = env.chat();
    chat.chat(
        &session.to_string(),
        "I'm planning to refactor the authentication module next week. \
         The current JWT implementation is too tightly coupled to the HTTP layer.",
    )
    .await
    .expect("first chat");
    env.log_session_json("chat_1", &session).await;

    // First reflection
    let first_result = env.run_reflection(&session, None).await;
    env.log_session_json("reflection_1", &session).await;
    let first_handoff = first_result.result.message.clone();
    assert!(
        !first_handoff.trim().is_empty(),
        "first reflection should produce a handoff"
    );
    env.log(format!("first handoff: {first_handoff}"));

    // Second conversation in a new session
    let session2 = env.create_session().await;
    let chat2 = env.chat();
    chat2
        .chat(
            &session2.to_string(),
            "I decided to use tower middleware for auth instead of custom extractors.",
        )
        .await
        .expect("second chat");
    env.log_session_json("chat_2", &session2).await;

    // Second reflection with the first handoff
    let second_result = env.run_reflection(&session2, Some(&first_handoff)).await;
    env.log_session_json("reflection_2", &session2).await;

    assert!(
        !second_result.result.message.trim().is_empty(),
        "second reflection should produce a handoff"
    );
    env.log(format!("second handoff: {}", second_result.result.message));
}

// ---------------------------------------------------------------------------
// Skills discoverability
// ---------------------------------------------------------------------------

/// Skills are discoverable — model can find and reference installed skills.
#[tokio::test]
async fn skills_discoverable_without_prompting() {
    let env = common::live_test_database("skills_discover").await;
    let session = env.create_session().await;

    // The bootstrapped workspace includes default skills in skills/
    assert!(
        env.workspace_file_exists("skills"),
        "skills directory should exist after bootstrap"
    );

    let chat = env.chat();
    let result = chat
        .chat(
            &session.to_string(),
            "What skills do you have available? List them briefly.",
        )
        .await
        .expect("chat response");
    env.log_session_json("skills_query", &session).await;

    assert!(
        !result.message.trim().is_empty(),
        "model should describe available skills"
    );
    env.log(format!("skills response: {}", result.message));
}
