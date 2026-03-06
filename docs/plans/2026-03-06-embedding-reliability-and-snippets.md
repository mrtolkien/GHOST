# Embedding Reliability & Search Snippet Quality

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this
> plan task-by-task.

**Goal:** Fix three embedding reliability issues (transactional persistence, watcher
resilience, periodic reconciliation) and improve search snippet quality using FTS5
snippet() function.

**Architecture:** Make embedding persistence atomic per-source so partial failures can't
poison the hash check. Make the file watcher resilient to Ollama downtime (sync files to
DB always, embed when available). Add hourly background reconciliation. Replace
first-line snippet extraction with FTS5's built-in `snippet()` for BM25 hits and prefer
embedding chunk text in hybrid merge.

**Tech Stack:** SQLite (sqlx transactions), tokio (spawn, interval), FTS5 snippet()
function

---

### Task 1: Transactional embedding persistence — DB function

**Files:**

- Modify: `src/db/embeddings.rs` (add `replace_embeddings_for_source`)
- Test: `tests/embeddings.rs`

The root cause of partial embeddings: each `upsert_embedding` call stores the content
hash independently. If the process errors after inserting 3 of 52 chunks, those 3 rows
have the correct hash — future reconciliation sees the hash match and skips the source,
leaving it permanently under-embedded.

Fix: a new function that deletes old + inserts all new chunks in a single transaction.

**Step 1: Write the failing test**

Add to `tests/embeddings.rs`:

```rust
#[tokio::test]
async fn replace_embeddings_atomically_swaps_all_chunks() {
    let (db, _config, _workspace, _config_dir) = common::test_database().await;

    let note_id =
        db::knowledge::create_note_full(&db, "Atomic Note", "body", &[], &[], 5, None, None)
            .await
            .expect("create note");

    // Insert 2 old chunks
    let old_vec = vec![0.1_f32; 1024];
    for i in 0..2 {
        db::embeddings::upsert_embedding(
            &db, "note", &note_id, i, &format!("old chunk {i}"), "old_hash", &old_vec, None,
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

    db::embeddings::replace_embeddings_for_source(
        &db, "note", &note_id, &chunks, "new_hash", None,
    )
    .await
    .expect("replace");

    // Should have exactly 3 chunks now
    assert_eq!(db::embeddings::count_embeddings(&db).await.unwrap(), 3);

    // Hash should be the new one
    let hash = db::embeddings::get_content_hash(&db, &note_id).await.unwrap();
    assert_eq!(hash.as_deref(), Some("new_hash"));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --test embeddings replace_embeddings_atomically -- --nocapture`
Expected: FAIL — `replace_embeddings_for_source` doesn't exist yet.

**Step 3: Implement `replace_embeddings_for_source`**

Add to `src/db/embeddings.rs`:

```rust
/// Atomically replace all embeddings for a source in a single transaction.
///
/// Deletes all existing chunks for `source_id`, then inserts all new chunks.
/// Either all chunks are persisted (with the new hash) or none are.
#[tracing::instrument(skip_all, fields(source_id = %source_id, chunks = chunks.len()))]
pub async fn replace_embeddings_for_source(
    db: &SqlitePool,
    source_table: &str,
    source_id: &str,
    chunks: &[(usize, String, Vec<f32>)],
    content_hash: &str,
    topic_id: Option<&str>,
) -> Result<(), DatabaseError> {
    let mut tx = db.begin().await.map_err(|source| DatabaseError::Query {
        table: "embedding",
        operation: "replace/begin",
        source,
    })?;

    // Delete old vec_embedding rows
    sqlx::query(
        "DELETE FROM vec_embedding WHERE rowid IN \
         (SELECT rowid FROM embedding WHERE source_id = ?)",
    )
    .bind(source_id)
    .execute(&mut *tx)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "vec_embedding",
        operation: "replace/delete_vec",
        source,
    })?;

    // Delete old embedding rows
    sqlx::query("DELETE FROM embedding WHERE source_id = ?")
        .bind(source_id)
        .execute(&mut *tx)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "embedding",
            operation: "replace/delete",
            source,
        })?;

    // Insert all new chunks
    for (chunk_index, chunk_text, vector) in chunks {
        let id = new_id();
        sqlx::query(
            "INSERT INTO embedding \
             (id, source_table, source_id, chunk_index, chunk_text, content_hash, topic_id, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(source_table)
        .bind(source_id)
        .bind(*chunk_index as i64)
        .bind(chunk_text)
        .bind(content_hash)
        .bind(topic_id)
        .bind(now())
        .execute(&mut *tx)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "embedding",
            operation: "replace/insert",
            source,
        })?;

        let (rowid,): (i64,) = sqlx::query_as("SELECT last_insert_rowid()")
            .fetch_one(&mut *tx)
            .await
            .map_err(|source| DatabaseError::Query {
                table: "embedding",
                operation: "replace/rowid",
                source,
            })?;

        let vec_json = serde_json::to_string(vector).unwrap_or_default();
        sqlx::query("INSERT INTO vec_embedding(rowid, embedding) VALUES (?, ?)")
            .bind(rowid)
            .bind(&vec_json)
            .execute(&mut *tx)
            .await
            .map_err(|source| DatabaseError::Query {
                table: "vec_embedding",
                operation: "replace/insert_vec",
                source,
            })?;
    }

    tx.commit().await.map_err(|source| DatabaseError::Query {
        table: "embedding",
        operation: "replace/commit",
        source,
    })?;

    Ok(())
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test --test embeddings replace_embeddings_atomically -- --nocapture`
Expected: PASS

**Step 5: Commit**

```
feat: add transactional replace_embeddings_for_source
```

---

### Task 2: Wire pipeline to use transactional persistence

**Files:**

- Modify: `src/embeddings/pipeline.rs` — `embed_source_inner` and `embed_sources`

Replace the delete-then-loop-upsert pattern with a single
`replace_embeddings_for_source` call.

**Step 1: Update `embed_source_inner`**

In `src/embeddings/pipeline.rs`, replace lines 101–126 of `embed_source_inner`:

```rust
async fn embed_source_inner(
    client: &EmbeddingClient,
    db: &GhostDb,
    source_table: &str,
    source_id: &str,
    content: &str,
    tags: &[String],
    hash: &str,
    topic_id: Option<&str>,
) -> Result<usize, PipelineError> {
    let chunks = chunk_content(content, tags, None);
    if chunks.is_empty() {
        return Ok(0);
    }

    let vectors = embed_chunks(client, &chunks).await?;

    let chunk_data: Vec<(usize, String, Vec<f32>)> = chunks
        .iter()
        .zip(vectors)
        .map(|(chunk, vec)| (chunk.index, chunk.text.clone(), vec))
        .collect();

    db::embeddings::replace_embeddings_for_source(
        db,
        source_table,
        source_id,
        &chunk_data,
        hash,
        topic_id,
    )
    .await?;

    Ok(chunk_data.len())
}
```

**Step 2: Update `embed_sources` (Phase 4)**

In the same file, replace the Phase 4 loop (lines 238–259):

```rust
    // Phase 4: distribute vectors back and persist atomically per source
    let mut total_embedded = 0usize;
    for (src, range) in prepared.iter().zip(ranges.iter()) {
        let src_vectors = &all_vectors[range.clone()];

        let chunk_data: Vec<(usize, String, Vec<f32>)> = src
            .chunks
            .iter()
            .zip(src_vectors.iter())
            .map(|(chunk, vec)| (chunk.index, chunk.text.clone(), vec.clone()))
            .collect();

        db::embeddings::replace_embeddings_for_source(
            db,
            &src.table,
            &src.id,
            &chunk_data,
            &src.hash,
            src.topic_id.as_deref(),
        )
        .await?;

        total_embedded += chunk_data.len();
    }
```

**Step 3: Run all embedding tests**

Run: `cargo test --test embeddings -- --nocapture` Expected: All PASS (existing tests
should still work since behavior is unchanged for happy path)

**Step 4: Commit**

```
fix: use transactional embedding persistence to prevent partial state
```

---

### Task 3: Make file watcher resilient to Ollama downtime

**Files:**

- Modify: `src/daemon/watcher.rs`

Currently the watcher exits permanently if Ollama is unavailable at startup (line
27–32). Change it to always run the file sync loop, and check Ollama availability per
batch.

**Step 1: Modify `spawn_watcher`**

Replace the early-exit guard and modify `process_batch` to check availability:

```rust
pub fn spawn_watcher(
    db: GhostDb,
    workspace: PathBuf,
    embeddings_config: EmbeddingsConfig,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let client = EmbeddingClient::new(&embeddings_config);

        // Removed: no longer exit if Ollama is unavailable at start.
        // The watcher always runs to sync file changes to DB.
        // Embedding happens opportunistically when Ollama is reachable.

        let (tx, mut rx) = mpsc::channel::<PathBuf>(256);

        let _watcher = match setup_watcher(&workspace, tx) {
            Ok(w) => w,
            Err(e) => {
                logfire::error!("failed to start file watcher", error = e.to_string(),);
                return;
            }
        };

        info!("file watcher started");

        let debounce = Duration::from_millis(500);

        loop {
            let mut changed_paths: HashSet<PathBuf> = HashSet::new();

            tokio::select! {
                path = rx.recv() => {
                    match path {
                        Some(p) => { changed_paths.insert(p); }
                        None => break,
                    }
                }
                _ = shutdown.changed() => break,
            }

            // Drain additional events within the debounce window
            tokio::time::sleep(debounce).await;
            while let Ok(path) = rx.try_recv() {
                changed_paths.insert(path);
            }

            process_batch(&db, &workspace, &client, &changed_paths).await;
        }

        info!("file watcher stopped");
    })
}
```

**Step 2: Make `process_batch` check Ollama availability before embedding**

Replace the embedding call in `process_batch` (line 113–117):

```rust
    // Phase 2: batch-embed all collected sources (skip if Ollama unavailable)
    if !embed_requests.is_empty() {
        if client.is_available().await {
            if let Err(e) =
                crate::embeddings::pipeline::embed_sources(client, db, embed_requests).await
            {
                logfire::warn!("batch embedding error", error = e.to_string());
            }
        } else {
            logfire::debug!(
                "Ollama unavailable — skipping embedding for {} sources (will catch up on reconciliation)",
                embed_requests.len(),
            );
        }
    }
```

**Step 3: Run `just ci`**

Expected: All checks and tests pass.

**Step 4: Commit**

```
fix: file watcher syncs to DB even when Ollama is unavailable
```

---

### Task 4: Periodic embedding reconciliation

**Files:**

- Modify: `src/daemon/watcher.rs` (add `spawn_reconciliation_loop`)
- Modify: `src/daemon/run.rs` (spawn the loop, track its handle)
- Modify: `src/daemon/mod.rs` if `spawn_reconciliation_loop` needs to be pub

Spawn a background task that runs `reconcile_embeddings` once per hour. It checks Ollama
availability before each run and skips silently if unavailable.

**Step 1: Add `spawn_reconciliation_loop` to `src/daemon/watcher.rs`**

```rust
const RECONCILE_INTERVAL: Duration = Duration::from_secs(60 * 60); // 1 hour

/// Periodically reconcile embeddings to catch missed file changes.
///
/// Runs `reconcile_embeddings` once per hour. Skips if Ollama is unavailable.
/// The hash check inside `reconcile_embeddings` makes this cheap when nothing changed.
#[tracing::instrument(name = "start reconciliation loop", skip_all)]
pub fn spawn_reconciliation_loop(
    db: GhostDb,
    embeddings_config: EmbeddingsConfig,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let client = EmbeddingClient::new(&embeddings_config);

        loop {
            tokio::select! {
                _ = tokio::time::sleep(RECONCILE_INTERVAL) => {}
                _ = shutdown.changed() => break,
            }

            if !client.is_available().await {
                logfire::debug!("Ollama unavailable — skipping periodic reconciliation");
                continue;
            }

            info!("running periodic embedding reconciliation");
            match crate::embeddings::pipeline::reconcile_embeddings(&client, &db).await {
                Ok((embedded, skipped)) => {
                    if embedded > 0 {
                        info!(embedded, skipped, "periodic reconciliation complete");
                    }
                }
                Err(e) => {
                    logfire::warn!(
                        "periodic reconciliation failed",
                        error = e.to_string(),
                    );
                }
            }
        }
    })
}
```

**Step 2: Wire it in `src/daemon/run.rs`**

Update `BootResult` to include the new handle, and spawn the loop after the watcher:

1. Change `BootResult` type alias — add another `JoinHandle<()>`:

```rust
type BootResult = (
    watch::Sender<bool>,
    JoinHandle<()>,       // watcher
    JoinHandle<()>,       // reconciliation loop
    JoinHandle<()>,       // scheduler
    Option<(DiscordSender, JoinHandle<()>)>,
    JoinHandle<()>,       // event handler
);
```

2. After spawning the watcher (line 93-98), spawn the reconciliation loop:

```rust
    let reconcile_handle = super::watcher::spawn_reconciliation_loop(
        db.clone(),
        config.embeddings.clone(),
        shutdown_rx.clone(),
    );
```

3. Update the return tuple and the `run()` function to await/join the new handle.

In `run()`, add `let _ = reconcile_handle.await;` alongside the other handle joins.

**Step 3: Run `just ci`**

Expected: All checks and tests pass.

**Step 4: Commit**

```
feat: add hourly periodic embedding reconciliation
```

---

### Task 5: FTS5 snippet() for BM25 search results

**Files:**

- Modify: `src/db/knowledge/search.rs` — `search_notes`, `search_references`,
  `search_diary`
- Modify: `src/db/knowledge/records.rs` — `truncate_snippet` stays but is no longer the
  primary snippet source for BM25
- Test: `tests/embeddings.rs` (hybrid merge tests)

Replace `truncate_snippet(&r.content, 150)` (first line of entire document) with FTS5's
`snippet()` which returns text around the matching terms.

**Step 1: Update `search_references` in `src/db/knowledge/search.rs`**

Replace the SQL query to use `snippet()` instead of fetching raw content:

```rust
pub async fn search_references(
    db: &SqlitePool,
    query: &str,
    limit: usize,
    topic_id: Option<&str>,
) -> Result<Vec<SearchHit>, DatabaseError> {
    #[derive(sqlx::FromRow)]
    struct RefSearchRow {
        id: String,
        topic_name: String,
        path: String,
        snippet: String,
        score: f64,
    }

    let fts_query = sanitize_fts_query(query);

    // snippet(table, column_idx, open_marker, close_marker, ellipsis, max_tokens)
    // Column 1 = content (0 = topic_name)
    let rows = if let Some(tid) = topic_id {
        sqlx::query_as::<_, RefSearchRow>(
            "SELECT r.id, COALESCE(t.name, r.topic_id) AS topic_name, r.path, \
             snippet(reference_fts, 1, '', '', '...', 24) AS snippet, \
             -bm25(reference_fts, 2.0, 1.0) AS score \
             FROM reference_fts \
             JOIN reference r ON r.rowid = reference_fts.rowid \
             LEFT JOIN topic t ON t.id = r.topic_id \
             WHERE reference_fts MATCH ? AND r.topic_id = ? \
             ORDER BY score DESC \
             LIMIT ?",
        )
        .bind(&fts_query)
        .bind(tid)
        .bind(limit as i64)
        .fetch_all(db)
        .await
    } else {
        sqlx::query_as::<_, RefSearchRow>(
            "SELECT r.id, COALESCE(t.name, r.topic_id) AS topic_name, r.path, \
             snippet(reference_fts, 1, '', '', '...', 24) AS snippet, \
             -bm25(reference_fts, 2.0, 1.0) AS score \
             FROM reference_fts \
             JOIN reference r ON r.rowid = reference_fts.rowid \
             LEFT JOIN topic t ON t.id = r.topic_id \
             WHERE reference_fts MATCH ? \
             ORDER BY score DESC \
             LIMIT ?",
        )
        .bind(&fts_query)
        .bind(limit as i64)
        .fetch_all(db)
        .await
    }
    .map_err(|source| DatabaseError::Query {
        table: "reference",
        operation: "search",
        source,
    })?;

    Ok(rows
        .into_iter()
        .map(|r| SearchHit {
            id: r.id,
            title: r.topic_name,
            snippet: r.snippet,
            score: r.score,
            kind: "reference".to_string(),
            path: Some(format!("references/{}", r.path)),
        })
        .collect())
}
```

**Step 2: Update `search_notes`**

Same pattern — use `snippet(note_fts, 1, '', '', '...', 24)` for the body column (column
1):

```rust
    let rows = sqlx::query_as::<_, NoteSearchRow>(
        "SELECT n.id, n.title, \
         snippet(note_fts, 1, '', '', '...', 24) AS body, \
         -bm25(note_fts, 2.0, 1.0) AS score \
         FROM note_fts \
         JOIN note n ON n.rowid = note_fts.rowid \
         WHERE note_fts MATCH ? \
         ORDER BY score DESC \
         LIMIT ?",
    )
```

Keep `NoteSearchRow.body` as the field name — it now contains the snippet text. The
mapping code already does `truncate_snippet(&r.body, 150)` which is fine as a safety
truncation on the snippet output.

**Step 3: Update `search_diary`**

Use `snippet(diary_fts, 0, '', '', '...', 24)` — diary_fts has only `body` at column 0:

```rust
    let rows = sqlx::query_as::<_, DiarySearchRow>(
        "SELECT d.id, d.date, \
         snippet(diary_fts, 0, '', '', '...', 24) AS body, \
         -bm25(diary_fts) AS score \
         FROM diary_fts \
         JOIN diary d ON d.rowid = diary_fts.rowid \
         WHERE diary_fts MATCH ? \
         ORDER BY score DESC \
         LIMIT ?",
    )
```

**Step 4: Run `just ci`**

Expected: All checks and tests pass. The hybrid_merge tests in `tests/embeddings.rs` use
hand-crafted `SearchHit` structs and don't query the DB, so they won't be affected.

**Step 5: Commit**

```
fix: use FTS5 snippet() for context-aware search snippets
```

---

### Task 6: Prefer embedding chunk snippet in hybrid merge

**Files:**

- Modify: `src/db/knowledge/search.rs` — `hybrid_merge`
- Test: `tests/embeddings.rs`

Currently in `hybrid_merge`, BM25 snippet takes priority (set first, embedding snippet
only fills empty). Now that BM25 snippets are better (FTS5 snippet()), we should still
prefer the embedding chunk snippet when available, since the chunk text is semantically
matched to the query and often more relevant.

**Step 1: Write test**

Add to `tests/embeddings.rs`:

```rust
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
        chunk_text: "[section: Game Rules > Break]\nThe BREAK occurs when...".to_string(),
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
```

**Step 2: Run test to verify it fails**

Run: `cargo test --test embeddings hybrid_merge_prefers_embedding -- --nocapture`
Expected: FAIL — currently BM25 snippet wins.

**Step 3: Update `hybrid_merge`**

In `src/db/knowledge/search.rs`, in the embedding hit loop (lines 270–283), overwrite
the snippet when the embedding provides chunk text:

```rust
    for hit in embedding_hits {
        let key = hit.source_id.clone();
        let chunk_snippet = truncate_snippet(&hit.chunk_text, 150);
        let entry = merged.entry(key).or_insert_with(|| SearchHit {
            id: hit.source_id.clone(),
            title: String::new(),
            snippet: chunk_snippet.clone(),
            score: 0.0,
            kind: hit.source_table.clone(),
            path: None,
        });
        entry.score += 0.6 * hit.score;
        // Prefer embedding chunk snippet — it's semantically matched to the query
        if !chunk_snippet.is_empty() {
            entry.snippet = chunk_snippet;
        }
    }
```

**Step 4: Run all tests**

Run: `cargo test --test embeddings -- --nocapture` Expected: All PASS (new test passes,
existing hybrid_merge tests still pass since they don't have conflicting snippets or the
expected values don't depend on snippet content).

Check the existing `hybrid_merge_combines_bm25_and_embedding_scores` test — it has
`snippet: "snippet"` for BM25 and `chunk_text: "chunk"` for embedding. After this
change, the merged snippet will be `"chunk"` instead of `"snippet"`. This test doesn't
assert on snippet content, so it's fine.

**Step 5: Run `just ci`**

**Step 6: Commit**

```
fix: prefer embedding chunk snippet in hybrid search results
```

---

### Task 7: Final verification

**Step 1: Run `just ci`**

Expected: All format, check, clippy, and test steps pass.

**Step 2: Manual smoke test (if running daemon)**

```bash
ghost knowledge reindex
ghost knowledge search "BREAK" --kind reference --topic ark-nova/rules
```

Verify the snippet now shows text around "BREAK" rather than "## MATHIAS WIGGE".

**Step 3: Commit any remaining fixes, then offer PR**
