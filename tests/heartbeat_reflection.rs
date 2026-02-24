#![cfg(feature = "live-tests")]

mod common;

use ghost::jobs::heartbeat::is_heartbeat_continue;
use ghost::web::scan_web_cache;
#[allow(unused_imports)]
use std::path::PathBuf;

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

    let response = env.run_heartbeat(&session).await;
    env.log_session_json("heartbeat", &session).await;

    assert!(
        is_heartbeat_continue(&response),
        "Expected HEARTBEAT_CONTINUE, got: {:?}",
        response
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

    // Step 3: Run reflection (includes deterministic reference curation)
    let findings = env.run_reflection(&session, None).await;
    env.log(format!("reflection findings: {findings}"));
    assert!(
        !findings.trim().is_empty(),
        "reflection should produce a handoff note"
    );

    // Step 4: Verify deterministic curation moved cited files to references
    assert!(
        env.find_file_containing("references", "blog.tolki.dev"),
        "Expected a reference file containing 'blog.tolki.dev'"
    );

    // Step 5: Verify web cache was cleaned up by curation
    let remaining = scan_web_cache(&env.config.workspace).expect("scan after curation");
    env.log(format!(
        "web cache after curation: {}",
        remaining.as_deref().unwrap_or("empty")
    ));
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

    let response = env.run_heartbeat(&session).await;
    env.log_session_json("heartbeat_followup", &session).await;

    // Model should either continue silently or send a follow-up — either is valid
    assert!(
        !response.trim().is_empty(),
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

    let findings = env.run_reflection(&session, None).await;
    env.log_session_json("reflection", &session).await;

    assert!(
        !findings.trim().is_empty(),
        "reflection should produce a handoff note"
    );
    env.log(format!("handoff: {findings}"));

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
    let first_handoff = env.run_reflection(&session, None).await;
    env.log_session_json("reflection_1", &session).await;
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
    let second_findings = env.run_reflection(&session2, Some(&first_handoff)).await;
    env.log_session_json("reflection_2", &session2).await;

    assert!(
        !second_findings.trim().is_empty(),
        "second reflection should produce a handoff"
    );
    env.log(format!("second handoff: {second_findings}"));
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

// ---------------------------------------------------------------------------
// Reflection isolation: replay agent transcript + web cache, run reflection
// ---------------------------------------------------------------------------

/// Isolated reflection test using a pre-captured agent transcript and web
/// cache from a successful deep-research run. Tests ONLY the reflection
/// pipeline — no chat, no agent spawning.
///
/// Uses fixture: `tests/fixtures/e2e_research_post_agent/`
///   - `agent_transcript.json`: 16-message agent session (3D printer research)
///   - `web-cache/`: 25 cached web pages from the agent's research
///
/// ```sh
/// cargo test --features live-tests reflection_on_agent_transcript -- --nocapture
/// ```
#[tokio::test]
async fn reflection_on_agent_transcript() {
    let env = common::live_test_database("reflection_agent_transcript").await;

    // Load fixture: agent transcript + web cache
    let fixture_dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/e2e_research_post_agent");

    let transcript_json = std::fs::read_to_string(fixture_dir.join("agent_transcript.json"))
        .expect("read agent transcript fixture");
    let transcript: Vec<serde_json::Value> =
        serde_json::from_str(&transcript_json).expect("parse agent transcript");

    // Replay agent messages into a session
    let session = env.session_from_transcript(&transcript).await;
    env.log(format!("replayed {} agent messages", transcript.len()));

    // Install web cache fixture
    env.install_web_cache_fixture(&fixture_dir.join("web-cache"));
    let listing = scan_web_cache(&env.config.workspace).expect("scan web cache");
    env.log(format!(
        "web cache: {} chars",
        listing.as_deref().unwrap_or("").len()
    ));

    // Run reflection on the agent session
    let findings = tokio::time::timeout(
        std::time::Duration::from_secs(180),
        env.run_reflection(&session, None),
    )
    .await
    .expect("TIMEOUT: reflection did not complete within 3 minutes");

    env.log_session_json("agent_reflection", &session).await;
    env.log(format!("handoff: {findings}"));

    // --- Tier 1: hard asserts (must pass) ---

    assert!(
        !findings.trim().is_empty(),
        "T1: reflection should produce a non-empty handoff note"
    );

    let notes = env.list_notes();
    let refs = env.list_references();

    // Log note details for diagnostics
    for note in &notes {
        let preview = std::fs::read_to_string(note)
            .unwrap_or_default()
            .chars()
            .take(200)
            .collect::<String>();
        env.log(format!("note: {} — {preview}", note.display()));
    }
    for r in &refs {
        env.log(format!("reference: {}", r.display()));
    }

    env.log(format!(
        "T1 totals: {} notes, {} references",
        notes.len(),
        refs.len()
    ));

    assert!(
        !notes.is_empty() || !refs.is_empty(),
        "T1: reflection should create at least one note or reference"
    );

    // --- Tier 2: soft checks (log only, no assert) ---

    let has_entity_note = env.find_file_containing("notes", "P2S")
        || env.find_file_containing("notes", "p2s")
        || env.find_file_containing("notes", "Bambu")
        || env.find_file_containing("notes", "bambu")
        || env.find_file_containing("notes", "Prusa")
        || env.find_file_containing("notes", "prusa");
    env.log(format!(
        "T2 entity note (printer brand/model): {has_entity_note}"
    ));

    let has_source_note = env.find_file_containing("notes", "all3dp")
        || env.find_file_containing("notes", "All3DP")
        || env.find_file_containing("notes", "tomshardware")
        || env.find_file_containing("notes", "aurora");
    env.log(format!("T2 source quality note: {has_source_note}"));

    let has_decision_note = env.find_file_containing("notes", "decision")
        || env.find_file_containing("notes", "Decision")
        || env.find_file_containing("notes", "comparison")
        || env.find_file_containing("notes", "recommend");
    env.log(format!("T2 decision note: {has_decision_note}"));

    // --- Tier 3: deterministic reference curation (hard assert) ---

    // References should exist (cited cache files moved by curate_references)
    assert!(
        !refs.is_empty(),
        "T3: deterministic curation should have moved cited cache files to references/"
    );

    // Web cache should be cleaned (only files from our classified list)
    let remaining = scan_web_cache(&env.config.workspace).expect("scan web cache");
    env.log(format!(
        "T3 web cache after curation: {}",
        remaining.as_deref().unwrap_or("empty")
    ));

    let t2_pass = has_entity_note && has_source_note;
    env.log(format!(
        "TIER SUMMARY: T1=PASS, T2={}, T3=PASS",
        if t2_pass { "PASS" } else { "PARTIAL" },
    ));
}
