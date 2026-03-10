use crate::helpers::live_test_database;

/// Test: import a PDF reference, verify it gets chunked, embedded, and is
/// searchable with relevant snippets.
#[tokio::test]
async fn test_ark_nova_import() {
    let env = live_test_database("ark_nova_import").await;
    let daemon = env.boot_daemon().await;

    let session_id = ghost::db::sessions::create_session(&daemon.db)
        .await
        .expect("create session");

    daemon
        .session_chat
        .chat(
            &session_id,
            "Import the Ark Nova rules for future reference",
            None,
            None,
        )
        .await
        .expect("chat failed");

    daemon.settle().await.expect("settle after chat");

    daemon.trigger_idle_agents().await;
    daemon.settle().await.expect("settle after reflection");

    let ref_count = ghost::db::knowledge::count_references(&daemon.db)
        .await
        .expect("count references");
    assert!(
        ref_count > 0,
        "expected at least one reference after import, got {ref_count}"
    );

    let embedding_count = ghost::db::embeddings::count_embeddings(&daemon.db)
        .await
        .expect("count embeddings");
    assert!(
        embedding_count >= 50,
        "expected 50+ embedding chunks, got {embedding_count}"
    );

    let results =
        ghost::db::knowledge::search_references(&daemon.db, "ark nova break rules", 10, None)
            .await
            .expect("reference search");

    let all_snippets: String = results
        .iter()
        .map(|r| r.snippet.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        all_snippets.to_lowercase().contains("break"),
        "search for 'ark nova break rules' should return snippets mentioning breaks.\n\
         Got {} results with snippets:\n{all_snippets}",
        results.len()
    );

    env.log_session_json("ark_nova_chat", &session_id).await;

    daemon.shutdown().await;
}
