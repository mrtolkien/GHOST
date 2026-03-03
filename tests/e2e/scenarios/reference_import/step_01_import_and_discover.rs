use std::time::Duration;

use crate::common;
use crate::e2e::harness;

/// Step 01: Import Dioxus tutorial docs via git, then ask GHOST about hooks.
///
/// Pre-populates references by calling `import_git` directly, then sends
/// a chat message to verify GHOST discovers and uses the imported content.
#[tokio::test]
async fn reference_import_step_01_import_and_discover() {
    let env = common::live_test_database("reference_import_step_01").await;
    let workspace_path = std::path::Path::new(&env.config.workspace);

    // --- Import Dioxus tutorial docs ---
    let import_config = ghost::reference_import::ImportConfig {
        source: ghost::reference_import::ImportSource::Git {
            url: "https://github.com/DioxusLabs/docsite".to_string(),
            paths: vec!["docs-src/0.7/src/tutorial/".to_string()],
            extensions: vec![".md".to_string()],
        },
        topic: "dioxus/docs".to_string(),
    };

    let result = ghost::reference_import::import_git(
        &env.db,
        workspace_path,
        &env.config.embeddings,
        &import_config,
    )
    .await
    .expect("git import should succeed");

    assert!(
        result.references_created > 0,
        "should import at least one reference"
    );

    // Verify references exist
    let refs = ghost::db::knowledge::list_references_by_topic(&env.db, Some("dioxus/docs"), 100)
        .await
        .expect("list refs");
    assert!(
        !refs.is_empty(),
        "references should be present after import"
    );

    // --- Chat: ask about hooks ---
    let session = env.create_session().await;
    let chat = env.chat();
    let (chat_result, _metadata) = tokio::time::timeout(
        Duration::from_secs(120),
        chat.chat(&session, "How do hooks work in Dioxus?", None),
    )
    .await
    .expect("TIMEOUT: chat should respond")
    .expect("chat failed");

    assert!(
        !chat_result.message.trim().is_empty(),
        "GHOST should return a non-empty response"
    );

    let tool_calls = env.collect_tool_calls(&session).await;
    assert!(
        tool_calls.iter().any(|t| t == "knowledge_search"),
        "GHOST should call knowledge_search, got: {tool_calls:?}"
    );

    // Save state for step 02
    let mut state = harness::fresh_step_state(
        harness::SCENARIO_REFERENCE_IMPORT,
        harness::STEP_RI_01,
        None,
        session,
    );
    state.assertion_markers.insert(
        "references_created".to_string(),
        serde_json::json!(result.references_created),
    );
    state
        .assertion_markers
        .insert("topic_id".to_string(), serde_json::json!(result.topic_id));
    state.final_response_preview = Some(truncate_preview(&chat_result.message));

    harness::save_step_snapshot(&env, &state).await;
}

fn truncate_preview(s: &str) -> String {
    let max = 300usize;
    if s.len() <= max {
        s.to_string()
    } else {
        let boundary = s.floor_char_boundary(max);
        format!("{}...", &s[..boundary])
    }
}
