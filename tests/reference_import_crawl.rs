#[cfg(feature = "live-tests")]
mod common;

/// Crawl ghost.tolki.dev with shallow depth and small page limit.
/// Verifies references are created, idempotent re-crawl, and cleanup.
#[tokio::test]
#[cfg(feature = "live-tests")]
async fn crawl_import_small_site() {
    use ghost::db;
    use ghost::reference_import::{ImportConfig, ImportSource};
    let env = common::live_test_database("crawl_import").await;
    let workspace_path = std::path::Path::new(&env.config.workspace);

    let max_pages = 5usize;
    let import_config = ImportConfig {
        source: ImportSource::Crawl {
            url: "https://ghost.tolki.dev/".to_string(),
            max_depth: 2,
            max_pages,
        },
        topic: "ghost/docs".to_string(),
    };

    // --- Phase 1: Initial crawl ---
    let result = ghost::reference_import::import_crawl(&env.db, workspace_path, &import_config)
        .await
        .expect("initial crawl");

    assert!(
        result.references_created > 0,
        "should create at least one reference"
    );
    assert!(
        result.references_created <= max_pages,
        "should not exceed max_pages ({}), got {}",
        max_pages,
        result.references_created
    );

    // Topic hierarchy should exist
    let topic = db::knowledge::find_topic_by_name(&env.db, "ghost/docs")
        .await
        .expect("find topic")
        .expect("topic exists");
    assert_eq!(result.topic_id, topic.id);

    // References should have source_url set
    let refs = db::knowledge::list_references_by_topic(&env.db, Some("ghost/docs"), 50)
        .await
        .expect("list refs");
    for r in &refs {
        assert!(
            r.source_url.is_some(),
            "each crawled reference should have source_url"
        );
    }

    // _import.toml should exist
    let import_toml = workspace_path.join("references/ghost/docs/_import.toml");
    assert!(import_toml.exists(), "_import.toml should exist");
    let toml_content = std::fs::read_to_string(&import_toml).expect("read _import.toml");
    assert!(
        toml_content.contains("source_type = \"crawl\""),
        "_import.toml should record crawl source type"
    );

    // --- Phase 2: Idempotent re-crawl ---
    let result2 = ghost::reference_import::import_crawl(&env.db, workspace_path, &import_config)
        .await
        .expect("re-crawl");

    assert_eq!(
        result2.references_created, 0,
        "re-crawl should create 0 new references"
    );
    assert!(
        result2.references_skipped > 0,
        "re-crawl should skip existing references"
    );

    // --- Phase 3: Generate embeddings (normally handled by file watcher) ---
    let client = ghost::embeddings::EmbeddingClient::new(&env.config.embeddings);
    {
        let refs = db::knowledge::list_references_by_topic(&env.db, Some(&topic.id), 50)
            .await
            .expect("list refs for embedding");
        let embed_requests: Vec<ghost::embeddings::pipeline::EmbedRequest> = refs
            .into_iter()
            .map(|r| ghost::embeddings::pipeline::EmbedRequest {
                source_table: "reference".into(),
                source_id: r.id,
                content: r.content,
                tags: vec![],
                topic_id: Some(topic.id.clone()),
                path: Some(r.path),
            })
            .collect();
        let embedded = ghost::embeddings::pipeline::embed_sources(&client, &env.db, embed_requests)
            .await
            .expect("embed references");
        assert!(embedded > 0, "should generate embeddings");
    }

    // --- Phase 4: Vector search returns hits ---
    let query_vectors = client
        .embed_batch(&["ghost configuration workspace".to_string()])
        .await
        .expect("embed query");
    assert!(!query_vectors.is_empty(), "should get query embedding");

    let topic_ids = vec![topic.id.clone()];
    let hits = db::embeddings::vector_search(&env.db, &query_vectors[0], 5, &topic_ids)
        .await
        .expect("vector search");
    assert!(
        !hits.is_empty(),
        "vector search scoped to ghost/docs should return hits from crawled content"
    );
    assert!(
        hits.iter().all(|h| h.source_table == "reference"),
        "all hits should be references"
    );
}
