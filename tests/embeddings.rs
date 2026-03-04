mod common;

use ghost::db;
use ghost::embeddings;

/// Read current process RSS from /proc/self/status (Linux only).
fn rss_mb() -> u64 {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            let kb: u64 = rest
                .trim()
                .trim_end_matches(" kB")
                .trim()
                .parse()
                .unwrap_or(0);
            return kb / 1024;
        }
    }
    0
}

// --- DB operations ---

#[tokio::test]
async fn upsert_and_count_embeddings() {
    let (db, _config, _workspace, _config_dir) = common::test_database().await;

    let note_id =
        db::knowledge::create_note_full(&db, "Test Note", "body", None, &[], &[], 5, None, None)
            .await
            .expect("create note");

    let vector = vec![0.1_f32; 1024];
    db::embeddings::upsert_embedding(
        &db,
        "note",
        &note_id,
        0,
        "chunk text",
        "abc123",
        &vector,
        None,
    )
    .await
    .expect("upsert embedding");

    let count = db::embeddings::count_embeddings(&db).await.expect("count");
    assert_eq!(count, 1);
}

#[tokio::test]
async fn upsert_overwrites_on_duplicate_source_and_chunk() {
    let (db, _config, _workspace, _config_dir) = common::test_database().await;

    let note_id =
        db::knowledge::create_note_full(&db, "Dup Note", "body", None, &[], &[], 5, None, None)
            .await
            .expect("create note");

    let vector_a = vec![0.1_f32; 1024];
    let vector_b = vec![0.9_f32; 1024];

    db::embeddings::upsert_embedding(
        &db, "note", &note_id, 0, "old text", "hash_a", &vector_a, None,
    )
    .await
    .expect("first upsert");
    db::embeddings::upsert_embedding(
        &db, "note", &note_id, 0, "new text", "hash_b", &vector_b, None,
    )
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

    let fake_id = "nonexistent";
    let hash = db::embeddings::get_content_hash(&db, fake_id)
        .await
        .expect("get hash");
    assert!(hash.is_none());
}

#[tokio::test]
async fn get_content_hash_returns_stored_hash() {
    let (db, _config, _workspace, _config_dir) = common::test_database().await;

    let note_id =
        db::knowledge::create_note_full(&db, "Hash Note", "body", None, &[], &[], 5, None, None)
            .await
            .expect("create note");

    let vector = vec![0.5_f32; 1024];
    db::embeddings::upsert_embedding(
        &db,
        "note",
        &note_id,
        0,
        "text",
        "my_hash_42",
        &vector,
        None,
    )
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
        db::knowledge::create_note_full(&db, "Multi Chunk", "body", None, &[], &[], 5, None, None)
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
            None,
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

    let note_a =
        db::knowledge::create_note_full(&db, "Note A", "body", None, &[], &[], 5, None, None)
            .await
            .expect("create a");
    let note_b =
        db::knowledge::create_note_full(&db, "Note B", "body", None, &[], &[], 5, None, None)
            .await
            .expect("create b");

    let vector = vec![0.1_f32; 1024];
    db::embeddings::upsert_embedding(&db, "note", &note_a, 0, "a", "h1", &vector, None)
        .await
        .unwrap();
    db::embeddings::upsert_embedding(&db, "note", &note_b, 0, "b", "h2", &vector, None)
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
        db::knowledge::create_note_full(&db, "Search Me", "body", None, &[], &[], 5, None, None)
            .await
            .expect("create note");

    // Insert a known vector
    let vector = vec![1.0_f32; 1024];
    db::embeddings::upsert_embedding(
        &db,
        "note",
        &note_id,
        0,
        "searchable chunk",
        "h",
        &vector,
        None,
    )
    .await
    .expect("upsert");

    // Search with an identical vector — should get a perfect match
    let hits = db::embeddings::vector_search(&db, &vector, 5, &[])
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

    let close_id =
        db::knowledge::create_note_full(&db, "Close", "body", None, &[], &[], 5, None, None)
            .await
            .unwrap();
    let far_id = db::knowledge::create_note_full(&db, "Far", "body", None, &[], &[], 5, None, None)
        .await
        .unwrap();

    // close_vec is similar to query_vec, far_vec is orthogonal
    let mut close_vec = vec![1.0_f32; 1024];
    close_vec[0] = 0.9;

    let mut far_vec = vec![0.0_f32; 1024];
    far_vec[0] = 1.0;

    db::embeddings::upsert_embedding(&db, "note", &close_id, 0, "close", "h1", &close_vec, None)
        .await
        .unwrap();
    db::embeddings::upsert_embedding(&db, "note", &far_id, 0, "far", "h2", &far_vec, None)
        .await
        .unwrap();

    let query_vec = vec![1.0_f32; 1024];
    let hits = db::embeddings::vector_search(&db, &query_vec, 5, &[])
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
            None,
        )
        .await
        .unwrap();
        db::embeddings::upsert_embedding(&db, "note", &id, 0, &format!("c{i}"), "h", &vector, None)
            .await
            .unwrap();
    }

    let hits = db::embeddings::vector_search(&db, &vector, 2, &[])
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
        id: "abc".to_string(),
        title: "BM25 Hit".to_string(),
        snippet: "snippet".to_string(),
        score: 1.0,
        kind: "note".to_string(),
        path: None,
    }];

    let embedding_hits = vec![db::embeddings::EmbeddingHit {
        source_id: "abc".to_string(),
        source_table: "note".to_string(),
        chunk_text: "chunk".to_string(),
        score: 0.8,
        topic_id: None,
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
        source_id: "xyz".to_string(),
        source_table: "note".to_string(),
        chunk_text: "only in embeddings".to_string(),
        score: 0.9,
        topic_id: None,
    }];

    let merged = db::knowledge::hybrid_merge(&bm25_hits, &embedding_hits, 10);
    assert_eq!(merged.len(), 1);
    assert!((merged[0].score - 0.6 * 0.9).abs() < 0.001);
}

#[test]
fn hybrid_merge_respects_limit() {
    let bm25_hits: Vec<db::knowledge::SearchHit> = (0..5)
        .map(|i| db::knowledge::SearchHit {
            id: format!("n{i}"),
            title: format!("Note {i}"),
            snippet: String::new(),
            score: 1.0,
            kind: "note".to_string(),
            path: None,
        })
        .collect();

    let merged = db::knowledge::hybrid_merge(&bm25_hits, &[], 3);
    assert_eq!(merged.len(), 3);
}

// --- Vector insert memory reproduction ---

/// Reproduces the production path: for each source, delete old embeddings then
/// insert new ones with 1024-dim vectors. Also includes large chunk_text to
/// simulate real reference documents. Measures RSS to detect memory
/// amplification.
///
/// This test checks whether the pattern stays bounded locally.
#[tokio::test]
async fn vector_insert_memory_stays_bounded() {
    let (db, _config, _workspace, _config_dir) = common::test_database().await;

    let before_mb = rss_mb();

    // Simulate realistic reference content (~4KB per chunk, like web pages)
    let large_chunk_text = "x".repeat(4000);

    // Phase 1: initial insert (like first reconciliation)
    let mut note_ids = Vec::new();
    for i in 0..50 {
        let note_id = db::knowledge::create_note_full(
            &db,
            &format!("MemTest Note {i}"),
            &format!("body of note {i}"),
            None,
            &[],
            &[],
            5,
            None,
            None,
        )
        .await
        .expect("create note");

        for chunk in 0..3 {
            let vector: Vec<f32> = (0..1024).map(|j| (i * 1024 + j) as f32 * 0.001).collect();
            db::embeddings::upsert_embedding(
                &db,
                "note",
                &note_id,
                chunk,
                &format!("{large_chunk_text} note {i} chunk {chunk}"),
                &format!("hash_{i}_{chunk}"),
                &vector,
                None,
            )
            .await
            .expect("upsert embedding");
        }
        note_ids.push(note_id);
    }

    let after_insert_mb = rss_mb();

    // Phase 2: delete + re-insert (like second reconciliation / re-embed)
    for (i, note_id) in note_ids.iter().enumerate() {
        db::embeddings::delete_embeddings_for_source(&db, note_id)
            .await
            .expect("delete");

        for chunk in 0..3 {
            let vector: Vec<f32> = (0..1024).map(|j| (i * 1024 + j) as f32 * 0.002).collect();
            db::embeddings::upsert_embedding(
                &db,
                "note",
                note_id,
                chunk,
                &format!("{large_chunk_text} note {i} chunk {chunk} v2"),
                &format!("hash_{i}_{chunk}_v2"),
                &vector,
                None,
            )
            .await
            .expect("re-upsert embedding");
        }
    }

    let count = db::embeddings::count_embeddings(&db).await.expect("count");
    assert_eq!(count, 150, "should have 150 embeddings after re-insert");

    let after_reinsert_mb = rss_mb();
    let total_delta_mb = after_reinsert_mb.saturating_sub(before_mb);

    eprintln!(
        "vector_insert_memory: before={before_mb}MB after_insert={after_insert_mb}MB \
         after_reinsert={after_reinsert_mb}MB total_delta={total_delta_mb}MB \
         (150 vectors x 1024 floats = 600KB raw vectors, ~600KB chunk text)"
    );

    assert!(
        total_delta_mb < 512,
        "RSS grew by {total_delta_mb}MB for 150 vectors (~1.2MB raw data) — \
         memory amplification ~{multiplier}x",
        multiplier = total_delta_mb * 1024 / 1200,
    );
}

/// Same as vector_insert_memory_stays_bounded but with concurrent database
/// operations (queries + inserts) to simulate the daemon environment where
/// Discord, scheduler, and watcher all share the same DB connection.
#[tokio::test]
#[ignore = "diagnostic: run with --ignored to test concurrent memory behavior"]
async fn vector_insert_concurrent_stays_bounded() {
    let (db, _config, _workspace, _config_dir) = common::test_database().await;

    let before_mb = rss_mb();
    let large_chunk_text = "x".repeat(4000);

    // Spawn concurrent "background" queries to simulate daemon subsystems
    let db_bg = db.clone();
    let background_queries = tokio::spawn(async move {
        for _ in 0..500 {
            // Simulate session/message queries that happen during Discord chat
            let _ = sqlx::query("SELECT count(*) FROM embedding")
                .fetch_optional(&db_bg)
                .await;
            let _ = sqlx::query("SELECT * FROM session LIMIT 10")
                .fetch_all(&db_bg)
                .await;
            let _ = sqlx::query("SELECT * FROM message ORDER BY created_at DESC LIMIT 20")
                .fetch_all(&db_bg)
                .await;
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    });

    // Insert embeddings concurrently from multiple "sources"
    let mut handles = Vec::new();
    for batch in 0..5 {
        let db_c = db.clone();
        let chunk_text = large_chunk_text.clone();
        handles.push(tokio::spawn(async move {
            for i in 0..10 {
                let idx = batch * 10 + i;
                let note_id = db::knowledge::create_note_full(
                    &db_c,
                    &format!("Concurrent Note {idx}"),
                    &format!("body {idx}"),
                    None,
                    &[],
                    &[],
                    5,
                    None,
                    None,
                )
                .await
                .expect("create note");

                for chunk in 0..3 {
                    let vector: Vec<f32> =
                        (0..1024).map(|j| (idx * 1024 + j) as f32 * 0.001).collect();
                    db::embeddings::upsert_embedding(
                        &db_c,
                        "note",
                        &note_id,
                        chunk,
                        &format!("{chunk_text} note {idx} chunk {chunk}"),
                        &format!("hash_{idx}_{chunk}"),
                        &vector,
                        None,
                    )
                    .await
                    .expect("upsert");
                }

                // Simulate Ollama round-trip delay
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        }));
    }

    for h in handles {
        h.await.expect("insert task");
    }
    background_queries.abort();

    // Phase 2: concurrent delete + re-insert (re-embed cycle)
    let all_notes: Vec<db::embeddings::EmbeddingHit> =
        db::embeddings::vector_search(&db, &vec![1.0_f32; 1024], 1000, &[])
            .await
            .unwrap_or_default();

    let unique_sources: std::collections::HashSet<String> = all_notes
        .iter()
        .map(|h| format!("{:?}", h.source_id))
        .collect();

    let db_re = db.clone();
    let db_bg2 = db.clone();
    let bg2 = tokio::spawn(async move {
        for _ in 0..200 {
            let _ = sqlx::query("SELECT source_id, chunk_text FROM embedding LIMIT 5")
                .fetch_all(&db_bg2)
                .await;
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    });

    // Re-embed all notes (delete + reinsert)
    let notes_page = db::knowledge::list_notes_page(&db_re, 0, 100)
        .await
        .unwrap();
    for (i, note) in notes_page.iter().enumerate() {
        db::embeddings::delete_embeddings_for_source(&db_re, &note.id)
            .await
            .expect("delete");
        for chunk in 0..3 {
            let vector: Vec<f32> = (0..1024).map(|j| (i * 1024 + j) as f32 * 0.002).collect();
            db::embeddings::upsert_embedding(
                &db_re,
                "note",
                &note.id,
                chunk,
                &format!("{large_chunk_text} note {i} chunk {chunk} v2"),
                &format!("hash_{i}_{chunk}_v2"),
                &vector,
                None,
            )
            .await
            .expect("re-upsert");
        }
    }

    bg2.abort();

    let count = db::embeddings::count_embeddings(&db).await.expect("count");
    let after_mb = rss_mb();
    let delta_mb = after_mb.saturating_sub(before_mb);

    eprintln!(
        "vector_concurrent_memory: before={before_mb}MB after={after_mb}MB \
         delta={delta_mb}MB embeddings={count} sources={}",
        unique_sources.len()
    );

    assert!(
        delta_mb < 512,
        "RSS grew by {delta_mb}MB with concurrent DB operations"
    );
}

/// Stress test: 500 notes × 5 chunks = 2500 embeddings to check for
/// non-linear memory growth at scale.
#[tokio::test]
#[ignore = "diagnostic: run with --ignored to test large-scale memory behavior"]
async fn vector_insert_large_scale_memory() {
    let (db, _config, _workspace, _config_dir) = common::test_database().await;

    let before_mb = rss_mb();
    let large_chunk_text = "x".repeat(4000);

    for i in 0..500 {
        let note_id = db::knowledge::create_note_full(
            &db,
            &format!("Scale Note {i}"),
            &format!("body {i}"),
            None,
            &[],
            &[],
            5,
            None,
            None,
        )
        .await
        .expect("create note");

        for chunk in 0..5 {
            let vector: Vec<f32> = (0..1024).map(|j| (i * 1024 + j) as f32 * 0.001).collect();
            db::embeddings::upsert_embedding(
                &db,
                "note",
                &note_id,
                chunk,
                &format!("{large_chunk_text} note {i} chunk {chunk}"),
                &format!("hash_{i}_{chunk}"),
                &vector,
                None,
            )
            .await
            .expect("upsert");
        }

        if (i + 1) % 100 == 0 {
            let cur_mb = rss_mb();
            let count = db::embeddings::count_embeddings(&db).await.unwrap();
            eprintln!(
                "  [{i}/500] rss={cur_mb}MB delta={}MB embeddings={count}",
                cur_mb.saturating_sub(before_mb)
            );
        }
    }

    let count = db::embeddings::count_embeddings(&db).await.unwrap();
    let after_mb = rss_mb();
    let delta_mb = after_mb.saturating_sub(before_mb);

    // 2500 vectors × 1024 floats × 4 bytes = 10 MB raw vectors
    // 2500 × 4000 bytes chunk text = 10 MB raw text
    // Total raw data ~20 MB
    eprintln!(
        "vector_large_scale: before={before_mb}MB after={after_mb}MB \
         delta={delta_mb}MB embeddings={count} (~20MB raw data)"
    );

    assert!(
        delta_mb < 1024,
        "RSS grew by {delta_mb}MB for {count} embeddings (~20MB raw data)"
    );
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
