#![cfg(feature = "live-tests")]

mod common;

use ghost::convert::git::convert_git;
use ghost::db;
use ghost::embeddings::EmbeddingClient;
use ghost::reference_import::{import_from_path, ImportProvenance};

/// Full end-to-end test: sparse git clone → reference creation → embeddings →
/// BM25 search → vector search → hybrid search → idempotent re-import.
///
/// Requires: network access (GitHub clone) + running Ollama server.
#[tokio::test]
async fn import_and_query_git_references() {
    let (db, config, _workspace, _config_dir) = common::test_database().await;
    let workspace_path = std::path::Path::new(&config.workspace);

    // --- Phase 1a: Convert to staging ---

    let staging_root = workspace_path.join(".staging");
    let convert_result = convert_git(
        &staging_root,
        "https://github.com/DioxusLabs/docsite",
        &["docs-src/0.7/src/tutorial/".to_string()],
        &[".md".to_string()],
        None,
    )
    .await
    .expect("convert git");

    // --- Phase 1b: Import from staging ---

    let provenance = ImportProvenance {
        source_type: Some("git".to_string()),
        source_url: Some("https://github.com/DioxusLabs/docsite".to_string()),
        version_ref: Some(convert_result.version_ref.clone()),
        git_ref: None,
    };

    let result = import_from_path(
        &db,
        workspace_path,
        &convert_result.staging_dir,
        "dioxus/docs",
        &provenance,
        None,
    )
    .await
    .expect("import from path");

    assert!(
        result.references_created > 0,
        "should create references, got 0"
    );
    // --- Phase 2: Verify topic + import batch metadata ---

    let topic = db::knowledge::find_topic_by_name(&db, "dioxus/docs")
        .await
        .expect("find topic")
        .expect("topic exists");

    let batch = db::knowledge::get_import_batch_by_topic(&db, &topic.id)
        .await
        .expect("get batch")
        .expect("batch exists");
    assert!(batch.version_ref.is_some(), "should have commit hash");
    assert_eq!(batch.source_type, "git");
    assert!(!batch.source_url.is_empty(), "should have source_url");

    // Parent topic "dioxus" should also exist
    let parent = db::knowledge::find_topic_by_name(&db, "dioxus")
        .await
        .expect("find parent")
        .expect("parent topic exists");
    assert_ne!(parent.id, topic.id);

    // Topic note file should exist on disk
    let note_path = workspace_path.join("notes").join("dioxus/docs/index.md");
    assert!(note_path.exists(), "topic note file should be written");

    let ref_count = db::knowledge::count_references_by_topic(&db, &topic.id)
        .await
        .expect("count refs");
    assert_eq!(ref_count as usize, result.references_created);

    // --- Phase 3: BM25 search scoped to topic ---

    let bm25_hits = db::knowledge::search_references(&db, "component", 10, Some(&topic.id))
        .await
        .expect("bm25 scoped search");
    assert!(
        !bm25_hits.is_empty(),
        "BM25 search for 'component' in dioxus/docs should return hits"
    );

    // Unscoped BM25 search should also find them
    let bm25_unscoped = db::knowledge::search_references(&db, "component", 10, None)
        .await
        .expect("bm25 unscoped search");
    assert!(
        !bm25_unscoped.is_empty(),
        "unscoped BM25 search should also return hits"
    );

    // Create a decoy topic — searching it should find nothing from dioxus
    let decoy_id = db::knowledge::find_or_create_topic(&db, "unrelated/empty")
        .await
        .expect("decoy topic");
    let bm25_decoy = db::knowledge::search_references(&db, "component", 10, Some(&decoy_id))
        .await
        .expect("bm25 decoy search");
    assert!(
        bm25_decoy.is_empty(),
        "search on unrelated topic should return nothing"
    );

    // --- Phase 3b: Generate embeddings (normally handled by file watcher) ---

    let client = EmbeddingClient::new(&config.embeddings);
    {
        let refs = db::knowledge::list_references_by_topic(&db, Some(&topic.id), 1000)
            .await
            .expect("list refs");
        let embed_requests: Vec<ghost::embeddings::pipeline::EmbedRequest> = refs
            .into_iter()
            .map(|r| ghost::embeddings::pipeline::EmbedRequest {
                source_table: "reference".into(),
                source_id: r.id,
                content: r.content,
                tags: vec![],
                topic_id: Some(topic.id.clone()),
                path: Some(r.path),
                reason: ghost::embeddings::pipeline::EmbedReason::New,
            })
            .collect();
        let embedded = ghost::embeddings::pipeline::embed_sources(&client, &db, embed_requests)
            .await
            .expect("embed references");
        assert!(embedded > 0, "should generate embeddings");
    }

    // --- Phase 4: Vector search (requires Ollama) ---
    assert!(
        client.is_available().await,
        "Ollama must be running for this test"
    );

    let query_vectors = client
        .embed_batch(&["dioxus component tutorial".to_string()])
        .await
        .expect("embed query");
    assert!(!query_vectors.is_empty(), "should get query embedding");
    let query_vec = &query_vectors[0];

    // Vector search scoped to topic
    let vec_hits =
        db::embeddings::vector_search(&db, query_vec, 10, std::slice::from_ref(&topic.id))
            .await
            .expect("vector search scoped");
    assert!(
        !vec_hits.is_empty(),
        "vector search scoped to dioxus/docs should return hits"
    );
    assert!(
        vec_hits.iter().all(|h| h.source_table == "reference"),
        "all vector hits should be references"
    );

    // Vector search scoped to wrong topic should return nothing
    let vec_decoy =
        db::embeddings::vector_search(&db, query_vec, 10, std::slice::from_ref(&decoy_id))
            .await
            .expect("vector search decoy");
    assert!(
        vec_decoy.is_empty(),
        "vector search on empty topic should return nothing"
    );

    // Unscoped vector search should find dioxus refs
    let vec_unscoped = db::embeddings::vector_search(&db, query_vec, 10, &[])
        .await
        .expect("vector search unscoped");
    assert!(
        !vec_unscoped.is_empty(),
        "unscoped vector search should return hits"
    );

    // --- Phase 5: Hybrid merge (BM25 + vector) ---

    let hybrid_hits = db::knowledge::hybrid_merge(&bm25_hits, &vec_hits, 10);
    assert!(
        !hybrid_hits.is_empty(),
        "hybrid merge should produce results"
    );
    // Hybrid scores should be positive (can exceed 1.0 when multiple chunks match)
    for hit in &hybrid_hits {
        assert!(
            hit.score > 0.0,
            "hybrid score {} should be positive",
            hit.score
        );
    }

    // --- Phase 6: Idempotent re-import ---

    let convert_result2 = convert_git(
        &staging_root,
        "https://github.com/DioxusLabs/docsite",
        &["docs-src/0.7/src/tutorial/".to_string()],
        &[".md".to_string()],
        None,
    )
    .await
    .expect("convert git re-import");

    let result2 = import_from_path(
        &db,
        workspace_path,
        &convert_result2.staging_dir,
        "dioxus/docs",
        &provenance,
        None,
    )
    .await
    .expect("re-import");

    assert_eq!(
        result2.references_created, 0,
        "re-import should create 0 new references"
    );
    assert_eq!(
        result2.references_skipped, result.references_created,
        "re-import should skip all previously created"
    );

    // --- Phase 7: Cleanup ---

    db::knowledge::delete_references_by_topic(&db, &topic.id)
        .await
        .expect("delete refs");

    let count_after = db::knowledge::count_references_by_topic(&db, &topic.id)
        .await
        .expect("count after delete");
    assert_eq!(count_after, 0, "all references should be deleted");

    // Embeddings should also be gone (cascaded)
    let vec_after_delete =
        db::embeddings::vector_search(&db, query_vec, 10, std::slice::from_ref(&topic.id))
            .await
            .expect("vector search after delete");
    assert!(
        vec_after_delete.is_empty(),
        "embeddings should be deleted with references"
    );
}
