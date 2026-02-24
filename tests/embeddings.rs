mod common;

use ghost::db;
use ghost::embeddings;

// --- DB operations ---

#[tokio::test]
async fn upsert_and_count_embeddings() {
    let (db, _config, _workspace, _config_dir) = common::test_database().await;

    let note_id =
        db::knowledge::create_note_full(&db, "Test Note", "body", None, &[], &[], 5, None)
            .await
            .expect("create note");

    let vector = vec![0.1_f32; 1024];
    db::embeddings::upsert_embedding(&db, "note", &note_id, 0, "chunk text", "abc123", &vector)
        .await
        .expect("upsert embedding");

    let count = db::embeddings::count_embeddings(&db).await.expect("count");
    assert_eq!(count, 1);
}

#[tokio::test]
async fn upsert_overwrites_on_duplicate_source_and_chunk() {
    let (db, _config, _workspace, _config_dir) = common::test_database().await;

    let note_id = db::knowledge::create_note_full(&db, "Dup Note", "body", None, &[], &[], 5, None)
        .await
        .expect("create note");

    let vector_a = vec![0.1_f32; 1024];
    let vector_b = vec![0.9_f32; 1024];

    db::embeddings::upsert_embedding(&db, "note", &note_id, 0, "old text", "hash_a", &vector_a)
        .await
        .expect("first upsert");
    db::embeddings::upsert_embedding(&db, "note", &note_id, 0, "new text", "hash_b", &vector_b)
        .await
        .expect("second upsert");

    let count = db::embeddings::count_embeddings(&db).await.expect("count");
    assert_eq!(count, 1, "duplicate upsert should not create a second row");

    let hash = db::embeddings::get_content_hash(&db, &note_id)
        .await
        .expect("get hash");
    assert_eq!(
        hash.as_deref(),
        Some("hash_b"),
        "hash should reflect latest upsert"
    );
}

#[tokio::test]
async fn get_content_hash_returns_none_for_missing_source() {
    let (db, _config, _workspace, _config_dir) = common::test_database().await;

    let fake_id = surrealdb::sql::Thing::from(("note", "nonexistent"));
    let hash = db::embeddings::get_content_hash(&db, &fake_id)
        .await
        .expect("get hash");
    assert!(hash.is_none());
}

#[tokio::test]
async fn get_content_hash_returns_stored_hash() {
    let (db, _config, _workspace, _config_dir) = common::test_database().await;

    let note_id =
        db::knowledge::create_note_full(&db, "Hash Note", "body", None, &[], &[], 5, None)
            .await
            .expect("create note");

    let vector = vec![0.5_f32; 1024];
    db::embeddings::upsert_embedding(&db, "note", &note_id, 0, "text", "my_hash_42", &vector)
        .await
        .expect("upsert");

    let hash = db::embeddings::get_content_hash(&db, &note_id)
        .await
        .expect("get hash");
    assert_eq!(hash.as_deref(), Some("my_hash_42"));
}

#[tokio::test]
async fn delete_embeddings_for_source_removes_all_chunks() {
    let (db, _config, _workspace, _config_dir) = common::test_database().await;

    let note_id =
        db::knowledge::create_note_full(&db, "Multi Chunk", "body", None, &[], &[], 5, None)
            .await
            .expect("create note");

    let vector = vec![0.1_f32; 1024];
    for i in 0..3 {
        db::embeddings::upsert_embedding(
            &db,
            "note",
            &note_id,
            i,
            &format!("chunk {i}"),
            "hash",
            &vector,
        )
        .await
        .expect("upsert");
    }

    assert_eq!(db::embeddings::count_embeddings(&db).await.unwrap(), 3);

    db::embeddings::delete_embeddings_for_source(&db, &note_id)
        .await
        .expect("delete for source");

    assert_eq!(db::embeddings::count_embeddings(&db).await.unwrap(), 0);
}

#[tokio::test]
async fn delete_all_embeddings_clears_table() {
    let (db, _config, _workspace, _config_dir) = common::test_database().await;

    let note_a = db::knowledge::create_note_full(&db, "Note A", "body", None, &[], &[], 5, None)
        .await
        .expect("create a");
    let note_b = db::knowledge::create_note_full(&db, "Note B", "body", None, &[], &[], 5, None)
        .await
        .expect("create b");

    let vector = vec![0.1_f32; 1024];
    db::embeddings::upsert_embedding(&db, "note", &note_a, 0, "a", "h1", &vector)
        .await
        .unwrap();
    db::embeddings::upsert_embedding(&db, "note", &note_b, 0, "b", "h2", &vector)
        .await
        .unwrap();

    assert_eq!(db::embeddings::count_embeddings(&db).await.unwrap(), 2);

    db::embeddings::delete_all_embeddings(&db)
        .await
        .expect("delete all");

    assert_eq!(db::embeddings::count_embeddings(&db).await.unwrap(), 0);
}

#[tokio::test]
async fn vector_search_returns_results() {
    let (db, _config, _workspace, _config_dir) = common::test_database().await;

    let note_id =
        db::knowledge::create_note_full(&db, "Search Me", "body", None, &[], &[], 5, None)
            .await
            .expect("create note");

    // Insert a known vector
    let vector = vec![1.0_f32; 1024];
    db::embeddings::upsert_embedding(&db, "note", &note_id, 0, "searchable chunk", "h", &vector)
        .await
        .expect("upsert");

    // Search with an identical vector — should get a perfect match
    let hits = db::embeddings::vector_search(&db, &vector, 5)
        .await
        .expect("vector search");

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].source_id, note_id);
    assert_eq!(hits[0].source_table, "note");
    assert_eq!(hits[0].chunk_text, "searchable chunk");
    assert!(
        hits[0].score > 0.99,
        "identical vector should have score ~1.0"
    );
}

#[tokio::test]
async fn vector_search_ranks_similar_higher() {
    let (db, _config, _workspace, _config_dir) = common::test_database().await;

    let close_id = db::knowledge::create_note_full(&db, "Close", "body", None, &[], &[], 5, None)
        .await
        .unwrap();
    let far_id = db::knowledge::create_note_full(&db, "Far", "body", None, &[], &[], 5, None)
        .await
        .unwrap();

    // close_vec is similar to query_vec, far_vec is orthogonal
    let mut close_vec = vec![1.0_f32; 1024];
    close_vec[0] = 0.9;

    let mut far_vec = vec![0.0_f32; 1024];
    far_vec[0] = 1.0;

    db::embeddings::upsert_embedding(&db, "note", &close_id, 0, "close", "h1", &close_vec)
        .await
        .unwrap();
    db::embeddings::upsert_embedding(&db, "note", &far_id, 0, "far", "h2", &far_vec)
        .await
        .unwrap();

    let query_vec = vec![1.0_f32; 1024];
    let hits = db::embeddings::vector_search(&db, &query_vec, 5)
        .await
        .expect("search");

    assert_eq!(hits.len(), 2);
    assert_eq!(
        hits[0].source_id, close_id,
        "close vector should rank first"
    );
    assert!(hits[0].score > hits[1].score);
}

#[tokio::test]
async fn vector_search_respects_limit() {
    let (db, _config, _workspace, _config_dir) = common::test_database().await;

    let vector = vec![1.0_f32; 1024];
    for i in 0..5 {
        let id = db::knowledge::create_note_full(
            &db,
            &format!("Limit Note {i}"),
            "body",
            None,
            &[],
            &[],
            5,
            None,
        )
        .await
        .unwrap();
        db::embeddings::upsert_embedding(&db, "note", &id, 0, &format!("c{i}"), "h", &vector)
            .await
            .unwrap();
    }

    let hits = db::embeddings::vector_search(&db, &vector, 2)
        .await
        .expect("search");
    assert_eq!(hits.len(), 2, "should respect limit=2");
}

#[tokio::test]
async fn count_embeddings_empty_table() {
    let (db, _config, _workspace, _config_dir) = common::test_database().await;

    let count = db::embeddings::count_embeddings(&db).await.expect("count");
    assert_eq!(count, 0);
}

// --- Hybrid merge ---

#[test]
fn hybrid_merge_combines_bm25_and_embedding_scores() {
    let bm25_hits = vec![db::knowledge::SearchHit {
        id: surrealdb::sql::Thing::from(("note", "abc")),
        title: "BM25 Hit".to_string(),
        snippet: "snippet".to_string(),
        score: 1.0,
        kind: "note".to_string(),
    }];

    let embedding_hits = vec![db::embeddings::EmbeddingHit {
        source_id: surrealdb::sql::Thing::from(("note", "abc")),
        source_table: "note".to_string(),
        chunk_text: "chunk".to_string(),
        score: 0.8,
    }];

    let merged = db::knowledge::hybrid_merge(&bm25_hits, &embedding_hits, 10);
    assert_eq!(merged.len(), 1);
    // 0.4 * (1.0/1.0) + 0.6 * 0.8 = 0.4 + 0.48 = 0.88
    let expected = 0.4 + 0.6 * 0.8;
    assert!((merged[0].score - expected).abs() < 0.001);
}

#[test]
fn hybrid_merge_includes_embedding_only_hits() {
    let bm25_hits = vec![];
    let embedding_hits = vec![db::embeddings::EmbeddingHit {
        source_id: surrealdb::sql::Thing::from(("note", "xyz")),
        source_table: "note".to_string(),
        chunk_text: "only in embeddings".to_string(),
        score: 0.9,
    }];

    let merged = db::knowledge::hybrid_merge(&bm25_hits, &embedding_hits, 10);
    assert_eq!(merged.len(), 1);
    assert!((merged[0].score - 0.6 * 0.9).abs() < 0.001);
}

#[test]
fn hybrid_merge_respects_limit() {
    let bm25_hits: Vec<db::knowledge::SearchHit> = (0..5)
        .map(|i| db::knowledge::SearchHit {
            id: surrealdb::sql::Thing::from(("note", format!("n{i}").as_str())),
            title: format!("Note {i}"),
            snippet: String::new(),
            score: 1.0,
            kind: "note".to_string(),
        })
        .collect();

    let merged = db::knowledge::hybrid_merge(&bm25_hits, &[], 3);
    assert_eq!(merged.len(), 3);
}

// --- Chunker ---

#[test]
fn content_hash_is_deterministic() {
    let hash_a = embeddings::pipeline::content_hash("hello world");
    let hash_b = embeddings::pipeline::content_hash("hello world");
    assert_eq!(hash_a, hash_b);
}

#[test]
fn content_hash_differs_for_different_content() {
    let hash_a = embeddings::pipeline::content_hash("hello");
    let hash_b = embeddings::pipeline::content_hash("world");
    assert_ne!(hash_a, hash_b);
}
