#![cfg(feature = "live-tests")]

mod common;

use ghost::db;
use ghost::embeddings::EmbeddingClient;
use ghost::embeddings::pipeline::{EmbedRequest, embed_sources};

#[tokio::test]
async fn embedding_client_is_available() {
    let (_db, config, _workspace, _config_dir) = common::test_database().await;
    let client = EmbeddingClient::new(&config.embeddings);

    let available = client.is_available().await;
    assert!(available, "Ollama server must be running");
}

#[tokio::test]
async fn embed_single_text() {
    let (_db, config, _workspace, _config_dir) = common::test_database().await;
    let client = EmbeddingClient::new(&config.embeddings);

    let vectors = client
        .embed_batch(&["hello world".to_string()])
        .await
        .expect("embed single text");

    assert_eq!(vectors.len(), 1);
    assert_eq!(
        vectors[0].len(),
        config.embeddings.dimension,
        "vector dimension should match config"
    );
}

#[tokio::test]
async fn embed_batch_multiple() {
    let (_db, config, _workspace, _config_dir) = common::test_database().await;
    let client = EmbeddingClient::new(&config.embeddings);

    let inputs: Vec<String> = (0..5)
        .map(|i| format!("test sentence number {i}"))
        .collect();
    let vectors = client.embed_batch(&inputs).await.expect("embed batch");

    assert_eq!(vectors.len(), 5);
    for v in &vectors {
        assert_eq!(v.len(), config.embeddings.dimension);
    }
}

#[tokio::test]
async fn embed_source_pipeline_stores_and_searches() {
    let (db, config, _workspace, _config_dir) = common::test_database().await;
    let client = EmbeddingClient::new(&config.embeddings);

    // Create a note in DB
    let note_id = db::knowledge::create_note_full(
        &db,
        "Dioxus Components",
        "Dioxus uses components as the basic building blocks of UI.",
        &["dioxus".to_string()],
        &[],
        5,
        None,
        None,
        None,
        None,
    )
    .await
    .expect("create note");

    // Embed it via the pipeline
    let requests = vec![EmbedRequest {
        source_table: "note".into(),
        source_id: note_id.clone(),
        content: "Dioxus uses components as the basic building blocks of UI.".into(),
        tags: vec!["dioxus".into()],
        topic_id: None,
        path: None,
    }];
    let chunks = embed_sources(&client, &db, requests)
        .await
        .expect("embed via pipeline");
    assert!(chunks > 0, "should embed at least one chunk");

    // Verify we can count the embedding
    let count = db::embeddings::count_embeddings(&db).await.expect("count");
    assert!(count > 0, "should have embeddings in DB");

    // Vector search should find the note
    let query_vec = client
        .embed_batch(&["dioxus component".to_string()])
        .await
        .expect("embed query");
    let hits = db::embeddings::vector_search(&db, &query_vec[0], 5, &[])
        .await
        .expect("vector search");

    assert!(!hits.is_empty(), "vector search should find the note");
    assert_eq!(hits[0].source_id, note_id);
    assert_eq!(hits[0].source_table, "note");
    assert!(hits[0].score > 0.5, "score should indicate relevance");
}
