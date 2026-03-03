use std::time::Duration;

use crate::e2e::harness;

/// Step 03: Crawl ghost.tolki.dev and ask about Ghost features.
///
/// Loads step 02 snapshot, runs a crawl import, then asks GHOST a
/// question that should be answered from the crawled content.
#[tokio::test]
async fn reference_import_step_03_crawl_and_search() {
    let loaded = harness::load_previous_step_or_fail(
        harness::SCENARIO_REFERENCE_IMPORT,
        harness::STEP_RI_03,
        harness::STEP_RI_02,
    )
    .await;
    let env = loaded.env;
    let workspace_path = std::path::Path::new(&env.config.workspace);

    // --- Crawl ghost.tolki.dev ---
    let import_config = ghost::reference_import::ImportConfig {
        source: ghost::reference_import::ImportSource::Crawl {
            url: "https://ghost.tolki.dev/".to_string(),
            max_depth: 2,
            max_pages: 5,
        },
        topic: "ghost/docs".to_string(),
    };

    let crawl_result = ghost::reference_import::import_crawl(
        &env.db,
        workspace_path,
        &env.config.embeddings,
        &import_config,
    )
    .await
    .expect("crawl import should succeed");

    assert!(
        crawl_result.references_created > 0 && crawl_result.references_created <= 5,
        "should crawl 1-5 pages, got {}",
        crawl_result.references_created,
    );

    // --- Chat: ask about Ghost features ---
    let session = env.create_session().await;
    let chat = env.chat();
    let (result, _metadata) = tokio::time::timeout(
        Duration::from_secs(120),
        chat.chat(&session, "What features does Ghost have?", None),
    )
    .await
    .expect("TIMEOUT: chat should respond")
    .expect("chat failed");

    assert!(
        !result.message.trim().is_empty(),
        "GHOST should return a non-empty response"
    );

    let tool_calls = env.collect_tool_calls(&session).await;
    assert!(
        tool_calls.iter().any(|t| t == "knowledge_search"),
        "GHOST should call knowledge_search, got: {tool_calls:?}"
    );

    let mut state = harness::fresh_step_state(
        harness::SCENARIO_REFERENCE_IMPORT,
        harness::STEP_RI_03,
        Some(harness::STEP_RI_02),
        session,
    );
    state.assertion_markers.insert(
        "crawl_references_created".to_string(),
        serde_json::json!(crawl_result.references_created),
    );
    state.final_response_preview = Some(truncate_preview(&result.message));

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
