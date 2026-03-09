# Working Embeddings Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Fix the embedding pipeline so imported references actually get chunked and embedded, and search snippets show relevant content instead of the first line of the file.

**Architecture:** Two independent bugs. (1) The file watcher misses reference files due to an inotify race: `create_dir_all` creates a new directory, `std::fs::write` writes a file into it immediately, but the `notify` crate hasn't set up a watch on the new directory yet — the file event is silently lost. Fix: when the watcher's batch contains directory paths, expand them into their file contents before processing. (2) `search_references` can't use FTS5 `snippet()` because `reference_fts` has a synthetic `topic_name` column that doesn't exist in the `reference` table. Fix: drop `topic_name` from `reference_fts` (topic search is already handled separately) so `snippet()` works, then use it.

**Tech Stack:** Rust, notify (inotify), SQLite FTS5, tree-sitter-md

---

## Bug 1: Watcher misses files in newly created directories

### Root cause

The import runs as a CLI subprocess (`ghost document import` via `run_shell_command`). It writes reference files to disk. The daemon's file watcher is supposed to pick them up. But:

```
create_dir_all("references/boardgames/arknova/")   // inotify CREATE on parent
std::fs::write("references/.../file.md")            // BEFORE notify adds watch on new dir
                                                     // → event LOST
```

The watcher batch contains directory paths (from inotify CREATE events on the parent), but `process_change` calls `read_to_string(directory_path)` which fails silently → `Ok(None)`.

### Task 1: Expand directory paths in watcher batch

**Files:**
- Modify: `src/daemon/watcher.rs:73-125` (the `process_batch` function)

**Step 1: Write the test**

There's no good way to unit-test the inotify race directly, but we can test the expansion helper. Add to `src/daemon/watcher.rs` at the bottom:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_directories_finds_nested_files() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("topic").join("subtopic");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("file.md"), "content").unwrap();
        std::fs::write(sub.join("_import.toml"), "meta").unwrap();
        std::fs::write(dir.path().join("root.md"), "root").unwrap();

        let mut paths = HashSet::new();
        paths.insert(dir.path().join("topic")); // directory, not a file
        paths.insert(dir.path().join("root.md")); // already a file

        let expanded = expand_directories(&paths);

        // Should contain the nested file and root.md, plus _import.toml
        assert!(expanded.contains(&sub.join("file.md")));
        assert!(expanded.contains(&sub.join("_import.toml")));
        assert!(expanded.contains(&dir.path().join("root.md")));
        // Should NOT contain the directory itself
        assert!(!expanded.contains(&dir.path().join("topic")));
    }

    #[test]
    fn expand_directories_handles_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let empty = dir.path().join("empty");
        std::fs::create_dir_all(&empty).unwrap();

        let mut paths = HashSet::new();
        paths.insert(empty.clone());

        let expanded = expand_directories(&paths);
        assert!(expanded.is_empty());
    }
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --lib "watcher::tests" -- --nocapture`
Expected: FAIL — `expand_directories` doesn't exist yet.

**Step 3: Implement `expand_directories`**

Add this function to `src/daemon/watcher.rs` (before `process_batch`):

```rust
/// Expand directory paths into their contained files.
///
/// When inotify reports a directory creation, files written into it
/// before the watch was established are missed. By expanding directory
/// paths, we catch those files.
fn expand_directories(paths: &HashSet<PathBuf>) -> HashSet<PathBuf> {
    let mut result = HashSet::new();
    for path in paths {
        if path.is_dir() {
            collect_files_recursive(path, &mut result);
        } else {
            result.insert(path.clone());
        }
    }
    result
}

fn collect_files_recursive(dir: &Path, out: &mut HashSet<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files_recursive(&path, out);
        } else if path.is_file() {
            out.insert(path);
        }
    }
}
```

**Step 4: Wire it into `process_batch`**

In `process_batch`, add the expansion as the first line of the function body, and use the expanded paths instead of the raw ones:

```rust
async fn process_batch(
    db: &GhostDb,
    workspace: &Path,
    client: &EmbeddingClient,
    paths: &HashSet<PathBuf>,
) {
    let paths = expand_directories(paths);

    // Phase 1: process each file ...
    // (rest of function unchanged, but now iterates over expanded `paths`)
```

Note: the `for path in paths` loop on line 83 needs no change — it already iterates the set. The shadowed `paths` binding replaces the original.

**Step 5: Run tests**

Run: `cargo test --lib "watcher::tests" -- --nocapture`
Expected: PASS

**Step 6: Run the e2e test**

Run: `cargo test --features live-tests test_ark_nova_import -- --nocapture`
Expected: ASSERT 2 should now pass (50+ embedding chunks). ASSERT 3 may still fail (snippet quality — addressed in Bug 2).

Note: this test takes ~2 minutes (real LLM + docling calls). If ASSERT 2 passes, the watcher fix works. If ASSERT 3 fails on snippet content, that's expected and addressed next.

**Step 7: Commit**

```
feat: expand directory paths in watcher to fix inotify race

When import tools create a new subdirectory and immediately write
a file into it, inotify misses the file event because the watch
hasn't been established on the new directory yet. Fix by expanding
any directory paths in the watcher batch into their file contents
before processing.
```

---

## Bug 2: Reference search snippets show first line instead of matching context

### Root cause

`reference_fts` has columns `(topic_name, content)` with `content=reference`. FTS5's `snippet()` requires the content table to have columns matching the FTS table's column names. The `reference` table has no `topic_name` column (it's synthesized from a JOIN in the trigger), so `snippet()` fails. The code falls back to `truncate_snippet(&r.content, 150)`, which takes only the **first line** of the content.

### Fix approach

Drop `topic_name` from `reference_fts`. Topic search is already handled by `topic_fts`. This lets `snippet()` work for the `content` column. Then use `snippet()` in the search query, matching how `note_fts` and `diary_fts` already work.

### Task 2: Migration to fix reference_fts

**Files:**
- Create: `migrations/NNN_fix_reference_fts.sql` (use the next migration number — check `ls migrations/`)

**Step 1: Write the migration**

Check existing migration numbers with `ls migrations/` and create the next one.

The migration drops and recreates `reference_fts` with only the `content` column, and rebuilds triggers:

```sql
-- Drop old triggers
DROP TRIGGER IF EXISTS reference_fts_ai;
DROP TRIGGER IF EXISTS reference_fts_ad;
DROP TRIGGER IF EXISTS reference_fts_au;

-- Drop and recreate FTS table without topic_name
DROP TABLE IF EXISTS reference_fts;
CREATE VIRTUAL TABLE reference_fts USING fts5(
    content,
    content=reference,
    content_rowid=rowid,
    tokenize='porter unicode61'
);

-- Rebuild triggers (no more topic_name lookup)
CREATE TRIGGER reference_fts_ai AFTER INSERT ON reference BEGIN
    INSERT INTO reference_fts(rowid, content)
    VALUES (new.rowid, new.content);
END;
CREATE TRIGGER reference_fts_ad AFTER DELETE ON reference BEGIN
    INSERT INTO reference_fts(reference_fts, rowid, content)
    VALUES ('delete', old.rowid, old.content);
END;
CREATE TRIGGER reference_fts_au AFTER UPDATE ON reference BEGIN
    INSERT INTO reference_fts(reference_fts, rowid, content)
    VALUES ('delete', old.rowid, old.content);
    INSERT INTO reference_fts(rowid, content)
    VALUES (new.rowid, new.content);
END;

-- Rebuild the FTS index from existing data
INSERT INTO reference_fts(reference_fts) VALUES ('rebuild');
```

**Step 2: Verify migration applies**

Run: `cargo test --test database -- --nocapture` (if there are DB tests) or just `cargo test --lib`
Expected: No errors.

**Step 3: Commit**

```
fix: drop topic_name from reference_fts to enable snippet()

reference_fts had a synthetic topic_name column that doesn't exist
in the reference table, which broke FTS5 snippet(). Topic search
is handled separately by topic_fts, so topic_name is not needed.
```

### Task 3: Use snippet() in search_references

**Files:**
- Modify: `src/db/knowledge/search.rs:74-147`

**Step 1: Write a test**

Add to `tests/embeddings.rs` (or `tests/database.rs`, whichever has reference search tests):

```rust
#[tokio::test]
async fn reference_search_snippet_contains_matched_term() {
    let env = test_database().await;

    // Create a reference with "break" deep in the content
    let topic_id = ghost::db::knowledge::find_or_create_topic(&env.db, "test-topic")
        .await
        .unwrap();
    ghost::db::knowledge::create_reference(
        &env.db,
        &topic_id,
        "test-topic/rules.md",
        "## Introduction\n\nThis is a long preamble about the game.\n\n## Break Rules\n\nDuring a break, players must discard down to their hand limit.",
        None,
        None,
    )
    .await
    .unwrap();

    let results = ghost::db::knowledge::search_references(&env.db, "break", 10, None)
        .await
        .unwrap();

    assert!(!results.is_empty(), "should find the reference");
    let snippet = &results[0].snippet;
    assert!(
        snippet.to_lowercase().contains("break"),
        "snippet should contain the matched term 'break', got: {snippet}"
    );
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test reference_search_snippet_contains_matched_term -- --nocapture`
Expected: FAIL — snippet currently returns "## Introduction" (first line).

**Step 3: Update search_references to use snippet()**

In `src/db/knowledge/search.rs`, update `search_references`:

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

    let rows = if let Some(tid) = topic_id {
        sqlx::query_as::<_, RefSearchRow>(
            "SELECT r.id, COALESCE(t.name, r.topic_id) AS topic_name, r.path, \
             snippet(reference_fts, 0, '', '', '...', 24) AS snippet, \
             -bm25(reference_fts, 1.0) AS score \
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
             snippet(reference_fts, 0, '', '', '...', 24) AS snippet, \
             -bm25(reference_fts, 1.0) AS score \
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
        .map(|r| {
            let snippet = truncate_snippet(&r.snippet, 150);
            SearchHit {
                id: r.id,
                title: r.topic_name,
                snippet,
                score: r.score,
                kind: "reference".to_string(),
                path: Some(format!("references/{}", r.path)),
            }
        })
        .collect())
}
```

Key changes:
- `bm25(reference_fts, 1.0)` — only 1 weight now (was `2.0, 1.0` for topic_name, content)
- `snippet(reference_fts, 0, '', '', '...', 24)` — column 0 is now `content`
- `RefSearchRow` has `snippet` instead of `content`
- Remove the comment about snippet() not working

**Step 4: Run the test**

Run: `cargo test reference_search_snippet_contains_matched_term -- --nocapture`
Expected: PASS

**Step 5: Run all tests**

Run: `just ci`
Expected: All pass.

**Step 6: Commit**

```
fix: use FTS5 snippet() for reference search results

Now that reference_fts has only the content column, snippet()
works correctly and returns context around matched terms instead
of the first line of the document.
```

### Task 4: Run the full e2e test

**Step 1: Run the e2e test**

Run: `cargo test --features live-tests test_ark_nova_import -- --nocapture`
Expected: All 3 assertions pass:
1. References created ✓
2. 50+ embedding chunks ✓
3. Search snippet contains "break" ✓

**Step 2: Commit (if any fixups needed)**

---

## Cleanup

### Task 5: Remove diagnostic test artifacts

The `tests/fixtures/ark_nova_docling.md` file was already removed. The three edge-case chunker tests added during investigation (`large_plain_text_no_headers_produces_multiple_chunks`, `single_very_long_line_produces_multiple_chunks`, `html_comments_only_content_chunks_properly`) are legitimate regression tests and should stay.

Run `just ci` one final time to confirm everything is green.
