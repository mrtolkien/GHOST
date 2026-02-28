#![cfg(feature = "live-tests")]

mod common;

use ghost::web::scan_web_cache;
use std::path::PathBuf;

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
        &session,
        "I just learned that SurrealDB supports graph relations using RELATE statements. \
         For example: `RELATE user:alice->follows->user:bob`. This creates typed edges \
         between records. I want to remember this for later.",
        None,
    )
    .await
    .expect("chat response");
    env.log_session_json("chat", &session).await;

    let (findings, _meta) = env.run_reflection(&session, None, "chat-reflection").await;
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
        &session,
        "I'm planning to refactor the authentication module next week. \
         The current JWT implementation is too tightly coupled to the HTTP layer.",
        None,
    )
    .await
    .expect("first chat");
    env.log_session_json("chat_1", &session).await;

    // First reflection (chat mode)
    let (first_handoff, _meta) = env.run_reflection(&session, None, "chat-reflection").await;
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
            &session2,
            "I decided to use tower middleware for auth instead of custom extractors.",
            None,
        )
        .await
        .expect("second chat");
    env.log_session_json("chat_2", &session2).await;

    // Second reflection with the first handoff (chat mode)
    let (second_findings, _meta) = env
        .run_reflection(&session2, Some(&first_handoff), "chat-reflection")
        .await;
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
    let (result, _metadata) = chat
        .chat(
            &session,
            "What skills do you have available? List them briefly.",
            None,
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

    // Run reflection on the agent session (agent mode: no diary instructions)
    let (findings, _meta) = tokio::time::timeout(
        std::time::Duration::from_secs(300),
        env.run_reflection(&session, None, "reflection"),
    )
    .await
    .expect("TIMEOUT: reflection did not complete within 5 minutes");

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

    // --- Tier 2: entity coverage (hard asserts) ---

    env.assert_notes_contain_any(&["P2S", "p2s"], "Bambu Lab P2S entity note");

    env.assert_notes_contain_any(
        &["Bambu", "bambu", "Prusa", "prusa"],
        "printer brand/model entity note",
    );

    env.assert_notes_contain_any(
        &["all3dp", "All3DP", "tomshardware", "aurora"],
        "source quality note",
    );

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

    env.log(format!(
        "TIER SUMMARY: T1=PASS, T2={}, T3=PASS",
        if has_decision_note {
            "PASS"
        } else {
            "PARTIAL (no decision note)"
        },
    ));
}
