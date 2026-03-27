mod common;

use ghost::db;
use ghost::db::knowledge::NoteInput;
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

    let note_id = db::knowledge::create_note_full(
        &db,
        &NoteInput {
            title: "Test Note",
            body: "body",
            trust: 5,
            ..Default::default()
        },
    )
    .await
    .expect("create note");

    let vector = vec![0.1_f32; 1024];
    db::embeddings::upsert_embedding(&db, "note", &note_id, 0, "chunk text", &vector, None)
        .await
        .expect("upsert embedding");

    let count = db::embeddings::count_embeddings(&db).await.expect("count");
    assert_eq!(count, 1);
}

#[tokio::test]
async fn upsert_overwrites_on_duplicate_source_and_chunk() {
    let (db, _config, _workspace, _config_dir) = common::test_database().await;

    let note_id = db::knowledge::create_note_full(
        &db,
        &NoteInput {
            title: "Dup Note",
            body: "body",
            trust: 5,
            ..Default::default()
        },
    )
    .await
    .expect("create note");

    let vector_a = vec![0.1_f32; 1024];
    let vector_b = vec![0.9_f32; 1024];

    db::embeddings::upsert_embedding(&db, "note", &note_id, 0, "old text", &vector_a, None)
        .await
        .expect("first upsert");
    db::embeddings::upsert_embedding(&db, "note", &note_id, 0, "new text", &vector_b, None)
        .await
        .expect("second upsert");

    let count = db::embeddings::count_embeddings(&db).await.expect("count");
    assert_eq!(count, 1, "duplicate upsert should not create a second row");
}

#[tokio::test]
async fn delete_embeddings_for_source_removes_all_chunks() {
    let (db, _config, _workspace, _config_dir) = common::test_database().await;

    let note_id = db::knowledge::create_note_full(
        &db,
        &NoteInput {
            title: "Multi Chunk",
            body: "body",
            trust: 5,
            ..Default::default()
        },
    )
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

    let note_a = db::knowledge::create_note_full(
        &db,
        &NoteInput {
            title: "Note A",
            body: "body",
            trust: 5,
            ..Default::default()
        },
    )
    .await
    .expect("create a");
    let note_b = db::knowledge::create_note_full(
        &db,
        &NoteInput {
            title: "Note B",
            body: "body",
            trust: 5,
            ..Default::default()
        },
    )
    .await
    .expect("create b");

    let vector = vec![0.1_f32; 1024];
    db::embeddings::upsert_embedding(&db, "note", &note_a, 0, "a", &vector, None)
        .await
        .unwrap();
    db::embeddings::upsert_embedding(&db, "note", &note_b, 0, "b", &vector, None)
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

    let note_id = db::knowledge::create_note_full(
        &db,
        &NoteInput {
            title: "Search Me",
            body: "body",
            trust: 5,
            ..Default::default()
        },
    )
    .await
    .expect("create note");

    // Insert a known vector
    let vector = vec![1.0_f32; 1024];
    db::embeddings::upsert_embedding(&db, "note", &note_id, 0, "searchable chunk", &vector, None)
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

    let close_id = db::knowledge::create_note_full(
        &db,
        &NoteInput {
            title: "Close",
            body: "body",
            trust: 5,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let far_id = db::knowledge::create_note_full(
        &db,
        &NoteInput {
            title: "Far",
            body: "body",
            trust: 5,
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // close_vec is similar to query_vec, far_vec is orthogonal
    let mut close_vec = vec![1.0_f32; 1024];
    close_vec[0] = 0.9;

    let mut far_vec = vec![0.0_f32; 1024];
    far_vec[0] = 1.0;

    db::embeddings::upsert_embedding(&db, "note", &close_id, 0, "close", &close_vec, None)
        .await
        .unwrap();
    db::embeddings::upsert_embedding(&db, "note", &far_id, 0, "far", &far_vec, None)
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
            &NoteInput {
                title: &format!("Limit Note {i}"),
                body: "body",
                trust: 5,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        db::embeddings::upsert_embedding(&db, "note", &id, 0, &format!("c{i}"), &vector, None)
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

#[test]
fn hybrid_merge_prefers_embedding_snippet() {
    let bm25_hits = vec![db::knowledge::SearchHit {
        id: "abc".to_string(),
        title: "Hit".to_string(),
        snippet: "bm25 snippet".to_string(),
        score: 1.0,
        kind: "reference".to_string(),
        path: None,
    }];

    let embedding_hits = vec![db::embeddings::EmbeddingHit {
        source_id: "abc".to_string(),
        source_table: "reference".to_string(),
        chunk_text: "The BREAK occurs when the round marker reaches the end".to_string(),
        score: 0.8,
        topic_id: None,
    }];

    let merged = db::knowledge::hybrid_merge(&bm25_hits, &embedding_hits, 10);
    assert_eq!(merged.len(), 1);
    assert!(
        merged[0].snippet.contains("BREAK"),
        "should prefer embedding chunk snippet, got: {}",
        merged[0].snippet,
    );
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
            &NoteInput {
                title: &format!("MemTest Note {i}"),
                body: &format!("body of note {i}"),
                trust: 5,
                ..Default::default()
            },
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

/// Insert 3 embedding chunks for a note (helper to reduce nesting in concurrent tests).
async fn upsert_chunks(db: &db::GhostDb, note_id: &str, idx: usize, chunk_text: &str) {
    for chunk in 0..3 {
        let vector: Vec<f32> = (0..1024).map(|j| (idx * 1024 + j) as f32 * 0.001).collect();
        db::embeddings::upsert_embedding(
            db,
            "note",
            note_id,
            chunk,
            &format!("{chunk_text} note {idx} chunk {chunk}"),
            &vector,
            None,
        )
        .await
        .expect("upsert");
    }
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
                    &NoteInput {
                        title: &format!("Concurrent Note {idx}"),
                        body: &format!("body {idx}"),
                        trust: 5,
                        ..Default::default()
                    },
                )
                .await
                .expect("create note");

                upsert_chunks(&db_c, &note_id, idx, &chunk_text).await;

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
    let notes_page = db::knowledge::list_all_notes(&db_re).await.unwrap();
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
            &NoteInput {
                title: &format!("Scale Note {i}"),
                body: &format!("body {i}"),
                trust: 5,
                ..Default::default()
            },
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

#[tokio::test]
async fn replace_embeddings_atomically_swaps_all_chunks() {
    let (db, _config, _workspace, _config_dir) = common::test_database().await;

    let note_id = db::knowledge::create_note_full(
        &db,
        &NoteInput {
            title: "Atomic Note",
            body: "body",
            trust: 5,
            ..Default::default()
        },
    )
    .await
    .expect("create note");

    // Insert 2 old chunks
    let old_vec = vec![0.1_f32; 1024];
    for i in 0..2 {
        db::embeddings::upsert_embedding(
            &db,
            "note",
            &note_id,
            i,
            &format!("old chunk {i}"),
            &old_vec,
            None,
        )
        .await
        .unwrap();
    }
    assert_eq!(db::embeddings::count_embeddings(&db).await.unwrap(), 2);

    // Replace with 3 new chunks atomically
    let new_vec = vec![0.9_f32; 1024];
    let chunks: Vec<(usize, String, Vec<f32>)> = (0..3)
        .map(|i| (i, format!("new chunk {i}"), new_vec.clone()))
        .collect();

    db::embeddings::replace_embeddings_for_source(&db, "note", &note_id, &chunks, None)
        .await
        .expect("replace");

    // Should have exactly 3 chunks now
    assert_eq!(db::embeddings::count_embeddings(&db).await.unwrap(), 3);
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

#[tokio::test]
async fn reconcile_filesystem_skips_unchanged_files() {
    let (db, _config, workspace, _config_dir) = common::test_database().await;

    // Write a note file to disk
    common::write_test_note(workspace.path(), "Hash Test", "initial content");

    // First reconciliation: file is new, should be processed
    let (discovered_1, _) = Box::pin(ghost::embeddings::pipeline::reconcile_filesystem(
        &db,
        workspace.path(),
    ))
    .await
    .unwrap();
    assert!(discovered_1 > 0, "first run should discover the new file");

    // Second reconciliation: file unchanged, hash matches but no embeddings yet
    // → should NOT re-discover but SHOULD queue embed request
    let (discovered_2, embed_reqs_2) = Box::pin(ghost::embeddings::pipeline::reconcile_filesystem(
        &db,
        workspace.path(),
    ))
    .await
    .unwrap();
    assert_eq!(
        discovered_2, 0,
        "second run should not re-discover unchanged file"
    );
    assert!(
        !embed_reqs_2.is_empty(),
        "should queue embed request for file without embeddings"
    );

    // Simulate embedding by inserting a dummy embedding for the note
    let note = db::knowledge::find_note_by_title(&db, "Hash Test")
        .await
        .unwrap()
        .expect("note should exist");
    let dummy_vec = vec![0.1_f32; 1024];
    db::embeddings::upsert_embedding(&db, "note", &note.id, 0, "chunk", &dummy_vec, None)
        .await
        .unwrap();

    // Third reconciliation: hash matches AND has embeddings → should skip entirely
    let (discovered_3, embed_reqs_3) = Box::pin(ghost::embeddings::pipeline::reconcile_filesystem(
        &db,
        workspace.path(),
    ))
    .await
    .unwrap();
    assert_eq!(discovered_3, 0, "third run should skip unchanged file");
    assert!(
        embed_reqs_3.is_empty(),
        "no embed requests when hash matches and embeddings exist"
    );

    // Modify the file
    common::write_test_note(workspace.path(), "Hash Test", "modified content");

    // Fourth reconciliation: file changed, hash differs → should be re-processed
    let (discovered_4, _) = Box::pin(ghost::embeddings::pipeline::reconcile_filesystem(
        &db,
        workspace.path(),
    ))
    .await
    .unwrap();
    assert!(
        discovered_4 > 0,
        "fourth run should re-process the modified file"
    );
}

#[tokio::test]
async fn reconcile_filesystem_queues_embed_for_unembedded_files() {
    let (db, _config, workspace, _config_dir) = common::test_database().await;

    // Write a note file and reconcile to create the DB record with file_hash
    common::write_test_note(workspace.path(), "Embed Gap", "embed me please");
    let (discovered, _) = Box::pin(ghost::embeddings::pipeline::reconcile_filesystem(
        &db,
        workspace.path(),
    ))
    .await
    .unwrap();
    assert!(discovered > 0);

    // Delete embeddings to simulate Ollama-was-down scenario
    let note = db::knowledge::find_note_by_title(&db, "Embed Gap")
        .await
        .unwrap()
        .expect("note should exist");
    db::embeddings::delete_embeddings_for_source(&db, &note.id)
        .await
        .unwrap();

    // Re-reconcile: hash matches but embeddings missing → should return EmbedRequest
    let (discovered_2, embed_reqs) = Box::pin(ghost::embeddings::pipeline::reconcile_filesystem(
        &db,
        workspace.path(),
    ))
    .await
    .unwrap();
    assert_eq!(
        discovered_2, 0,
        "file unchanged, should not count as discovered"
    );
    assert!(
        !embed_reqs.is_empty(),
        "should queue embed request for file with missing embeddings"
    );
    assert_eq!(embed_reqs[0].source_table, "note");
}

#[tokio::test]
async fn reconcile_filesystem_discovers_untracked_reference() {
    let (db, _config, workspace, _config_dir) = common::test_database().await;

    // Write a reference file to disk without creating a DB record
    let refs_dir = workspace.path().join("references").join("test-topic");
    std::fs::create_dir_all(&refs_dir).unwrap();
    std::fs::write(
        refs_dir.join("orphan.md"),
        "## Orphan Reference\n\nThis file exists on disk but not in the DB.",
    )
    .unwrap();

    // Verify it's not in the DB yet
    let count_before = db::knowledge::count_references(&db).await.unwrap();
    assert_eq!(count_before, 0);

    // Run filesystem reconciliation
    let (discovered, _embed_reqs) = Box::pin(ghost::embeddings::pipeline::reconcile_filesystem(
        &db,
        workspace.path(),
    ))
    .await
    .unwrap();

    assert!(discovered > 0, "should discover the orphan file");

    // Verify it's now in the DB
    let count_after = db::knowledge::count_references(&db).await.unwrap();
    assert!(count_after > 0, "orphan reference should now be in DB");
}
