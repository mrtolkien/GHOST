# Fast Boot Reconciliation — Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if
> subagents available) or superpowers:executing-plans to implement this plan. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reduce boot reconciliation from ~3s (150 files) to <200ms by adding file-level
hashing, skipping unchanged files, and removing redundant per-chunk hash checks.

**Architecture:** Add a `file_hash TEXT` column to `note`, `reference`, `diary`, `script`
tables. On boot, bulk-load `(path, file_hash, has_embeddings)` into memory via LEFT JOIN,
walk the filesystem, skip unchanged files, and only process/embed changed ones. Remove the
now-redundant `content_hash` column from the `embedding` table.

**Tech Stack:** SQLite (sqlx migrations), tokio::fs, SHA-256 (sha2 crate, already a dependency)

**Pre-existing bug (out of scope but noted):** `process_diary_change` in `watcher.rs`
never updates an existing diary's body — it finds by date and returns the old ID without
writing. The embeddings end up correct (uses file content), but the DB body is stale. A
follow-up should add `update_diary` logic, but this plan doesn't address it.

---

## Chunk 1: Migration and Data Layer

### Task 1: Add `file_hash` column to knowledge tables

**Files:**
- Create: `migrations/009_file_hash.sql`

- [ ] **Step 1: Write the migration**

```sql
ALTER TABLE note ADD COLUMN file_hash TEXT;
ALTER TABLE reference ADD COLUMN file_hash TEXT;
ALTER TABLE diary ADD COLUMN file_hash TEXT;
ALTER TABLE script ADD COLUMN file_hash TEXT;
```

Nullable — existing rows get NULL, which means "needs processing" (same as hash mismatch).

- [ ] **Step 2: Verify migration applies**

Run: `cargo build`
Expected: compiles (sqlx offline mode, so we need to update sqlx-data too)

- [ ] **Step 3: Commit**

```
feat: add file_hash column to knowledge tables
```

---

### Task 2: Update struct definitions and CRUD to include `file_hash`

**Files:**
- Modify: `src/db/knowledge/records.rs` — add `file_hash: Option<String>` to all 4 record structs
- Modify: `src/db/knowledge/crud.rs` — update `create_note_full`, `update_note`, `create_reference`, `create_script`, `update_script`, diary create/update to accept and store `file_hash`; update all SELECT queries to include `file_hash`

- [ ] **Step 1: Add `file_hash` field to record structs**

Add `pub file_hash: Option<String>` to `NoteRecord`, `ReferenceRecord`, `DiaryRecord`,
`ScriptRecord` in `records.rs`.

- [ ] **Step 2: Verify SELECT queries work automatically**

All existing queries use `SELECT * FROM note ...` etc. with `sqlx::FromRow` derivation.
Adding the field to the struct is sufficient — `FromRow` maps by column name, so `SELECT
*` picks up the new column automatically. **No query changes needed.**

- [ ] **Step 3: Update write functions to accept and store `file_hash`**

For `create_note_full` and `update_note`: add `file_hash: Option<&str>` parameter, bind
it in the INSERT/UPDATE query.

Same for `create_reference`, `update_reference` (if exists), `create_diary`/update,
`create_script`/`update_script`.

- [ ] **Step 4: Run `just ci`**

Fix all compilation errors from callers that now need the new parameter. Pass `None` at
all existing call sites for now — the reconciliation code will pass real hashes in Task 4.

- [ ] **Step 5: Commit**

```
feat: plumb file_hash through knowledge CRUD layer
```

---

### Task 3: Add bulk hash-loading query

**Files:**
- Modify: `src/db/knowledge/crud.rs` — add `load_file_hashes` function

This is the key query for the batch approach. One query per table, LEFT JOIN on embedding
to detect missing embeddings.

- [ ] **Step 1: Define the return type and function**

```rust
/// Lightweight record for boot reconciliation: just path + hash + embedding status.
pub struct FileHashRecord {
    pub path: String,
    pub file_hash: Option<String>,
    pub has_embeddings: bool,
}

/// Load all (path, file_hash, has_embeddings) for notes.
pub async fn load_note_file_hashes(
    db: &SqlitePool,
) -> Result<Vec<FileHashRecord>, DatabaseError> {
    let rows = sqlx::query_as!(
        FileHashRecord,
        r#"
        SELECT
            n.path as "path!",
            n.file_hash,
            (e.source_id IS NOT NULL) as "has_embeddings!: bool"
        FROM note n
        LEFT JOIN (
            SELECT DISTINCT source_id FROM embedding WHERE source_table = 'note'
        ) e ON e.source_id = n.id
        WHERE n.path IS NOT NULL
        "#,
    )
    .fetch_all(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "note",
        operation: "load_note_file_hashes",
        source,
    })?;
    Ok(rows)
}
```

Write similar functions for references, diary, and scripts:

- **References**: stored path lacks `references/` prefix. The caller prepends it when
  building the HashMap key: `format!("references/{}", r.path)`.
- **Diary**: has no `path` column — use `date` as the lookup key. The `FileHashRecord`
  `path` field will contain the date string (e.g. `"2026-03-14"`), and the caller builds
  the HashMap key as `format!("diary/{}.md", r.path)`.
- **Scripts**: stored path lacks `scripts/` prefix. Same pattern as references.

- [ ] **Step 2: Run `just ci`**

- [ ] **Step 3: Commit**

```
feat: add bulk file hash loading queries for boot reconciliation
```

---

## Chunk 2: Rewrite Reconciliation Pipeline

### Task 4: Rewrite `reconcile_filesystem` with batch hash checking

**Files:**
- Modify: `src/embeddings/pipeline.rs` — rewrite `reconcile_filesystem`
- Modify: `src/daemon/watcher.rs` — update `process_note_change` and siblings to accept
  and return `file_hash`, use `tokio::fs::read` instead of `std::fs::read_to_string`

The new flow:

```
1. Bulk load (path → file_hash, has_embeddings) from DB for all 4 tables
2. Walk filesystem, read each file (tokio::fs), compute SHA-256 of raw content
3. If path in DB map AND hash matches AND has_embeddings → skip entirely
4. If path in DB map AND hash matches AND !has_embeddings → queue EmbedRequest only
   (read file to get content for embedding, but skip DB upsert)
5. If hash differs or path missing → full process_change + store new file_hash
```

- [ ] **Step 1: Update `process_note_change` to use `tokio::fs::read_to_string`**

Replace `std::fs::read_to_string(path)` (line 258 of watcher.rs) with
`tokio::fs::read_to_string(path).await`. Do the same in `process_reference_change`,
`process_diary_change`, `process_script_change`.

- [ ] **Step 2: Update `process_note_change` to accept and store `file_hash`**

Add a `file_hash: &str` parameter. Pass it through to `create_note_full` / `update_note`.
Do the same for all 4 `process_*_change` functions.

- [ ] **Step 3: Rewrite `reconcile_filesystem`**

```rust
pub async fn reconcile_filesystem(
    db: &GhostDb,
    workspace: &std::path::Path,
) -> Result<(usize, Vec<EmbedRequest>), PipelineError> {
    // Phase 1: Load existing hashes from DB
    let note_hashes = db::knowledge::load_note_file_hashes(db).await?;
    let ref_hashes = db::knowledge::load_reference_file_hashes(db).await?;
    let diary_hashes = db::knowledge::load_diary_file_hashes(db).await?;
    let script_hashes = db::knowledge::load_script_file_hashes(db).await?;

    let mut known: HashMap<String, (Option<String>, bool)> = HashMap::new();
    for r in note_hashes { known.insert(r.path, (r.file_hash, r.has_embeddings)); }
    for r in ref_hashes { known.insert(format!("references/{}", r.path), (r.file_hash, r.has_embeddings)); }
    for r in diary_hashes { known.insert(format!("diary/{}.md", r.path), (r.file_hash, r.has_embeddings)); }
    for r in script_hashes { known.insert(format!("scripts/{}", r.path), (r.file_hash, r.has_embeddings)); }

    // Phase 2: Walk filesystem, check hashes, process changed files
    let mut discovered = 0usize;
    let mut embed_requests = Vec::new();

    for subdir in ["notes", "references", "diary", "scripts"] {
        let dir = workspace.join(subdir);
        if !dir.exists() { continue; }
        let files = walk_directory(&dir);
        for file_path in files {
            let rel = file_path.strip_prefix(workspace).unwrap_or(&file_path);
            let rel_str = rel.to_string_lossy().to_string();

            // Read file and compute hash
            let raw = match tokio::fs::read_to_string(&file_path).await {
                Ok(r) => r,
                Err(_) => continue,
            };
            let hash = content_hash(&raw);

            match known.get(&rel_str) {
                // Hash matches and has embeddings → skip entirely
                Some((Some(stored_hash), true)) if *stored_hash == hash => continue,
                // Hash matches but missing embeddings → need to re-embed only
                Some((Some(stored_hash), false)) if *stored_hash == hash => {
                    // Build EmbedRequest without re-upserting to DB
                    // (need to read enough to build the request — source_id, etc.)
                    // ... queue embed request from DB record ...
                }
                // Hash differs or missing → full process
                _ => {
                    match process_change(db, workspace, &file_path, &hash).await {
                        Ok(Some(req)) => {
                            embed_requests.push(req);
                            discovered += 1;
                        }
                        Ok(None) => {}
                        Err(e) => { /* warn */ }
                    }
                }
            }
        }
    }

    // Phase 3: Detect orphan DB records (file deleted from disk)
    for (path, _) in &known {
        let full_path = workspace.join(path);
        if !full_path.exists() {
            // Delete DB record + embeddings via process_change (handles deletion)
            let _ = process_change_deleted(db, workspace, &full_path).await;
        }
    }

    tracing::Span::current().record("discovered", discovered as u64);
    Ok((discovered, embed_requests))
}
```

Note: the "hash matches, no embeddings" branch needs the source_id and content to build
an `EmbedRequest`. Two options:
- (a) Also load `id` and `body`/`content` in the bulk query — more memory but no extra
  DB hit
- (b) Do a single DB lookup for just that record — rare case, acceptable

Option (b) is simpler since this path is uncommon (only when Ollama was previously down).
Use the existing `find_note_by_path` etc.

- [ ] **Step 4: Update `process_change` to accept pre-read content**

Since `reconcile_filesystem` already reads each file for hashing, pass the raw content
and hash through to avoid a second read:

```rust
pub async fn process_change(
    db: &GhostDb,
    workspace: &Path,
    path: &Path,
    raw_content: &str,
    file_hash: &str,
) -> Result<Option<EmbedRequest>, PipelineError>
```

The real-time watcher still reads files itself, so it calls this with freshly-read
content. Extract a `process_change_deleted` helper for the deletion path (no content
needed).

- [ ] **Step 5: Run `just ci`**

- [ ] **Step 6: Commit**

```
feat: batch hash-check in reconcile_filesystem, skip unchanged files
```

---

### Task 5: Merge `reconcile_embeddings` into `reconcile_filesystem`

**Files:**
- Modify: `src/embeddings/pipeline.rs` — remove standalone `reconcile_embeddings`, fold
  its purpose into the new flow
- Modify: `src/daemon/run.rs` — update boot sequence

The old flow was:
1. `reconcile_filesystem` → sync orphan files to DB (no embedding)
2. `reconcile_embeddings` → page through all DB records, hash-check, embed stale ones

The new flow is:
1. `reconcile_filesystem` → returns `Vec<EmbedRequest>` for changed/unembedded files
2. Call `embed_sources` on that Vec (already exists, handles batching)

- [ ] **Step 1: Update boot sequence in `src/daemon/run.rs`**

Replace:
```rust
reconcile_filesystem(&db, &config.workspace).await;
reconcile_embeddings(&client, &db).await;
```
With:
```rust
let (discovered, embed_requests) = reconcile_filesystem(&db, &config.workspace).await?;
if !embed_requests.is_empty() {
    embed_sources(&client, &db, embed_requests).await?;
}
```

- [ ] **Step 2: Remove `reconcile_embeddings`**

Delete the function from `pipeline.rs`. Also remove `list_notes_page`,
`list_references_page`, `list_diary_page`, `list_scripts_page` from `crud.rs` IF they
have no other callers (grep first).

- [ ] **Step 3: Update the hourly reconciliation loop**

In `spawn_reconciliation_loop` (watcher.rs ~line 546), apply the same change: call
`reconcile_filesystem` (which now returns embed requests) + `embed_sources`.

- [ ] **Step 4: Run `just ci`**

- [ ] **Step 5: Commit**

```
refactor: merge reconcile_embeddings into reconcile_filesystem flow
```

---

## Chunk 3: Remove Redundant Embedding Hash

### Task 6: Remove `content_hash` from embedding table

**Files:**
- Create: `migrations/010_drop_embedding_content_hash.sql`
- Modify: `src/db/embeddings.rs` — remove `content_hash` from all queries
- Modify: `src/embeddings/pipeline.rs` — remove `content_hash` parameter threading

- [ ] **Step 1: Write migration**

SQLite doesn't support `ALTER TABLE DROP COLUMN` before 3.35.0, but sqlx's bundled
SQLite is recent enough. Verify with:

```
sqlite3 --version
```

If ≥3.35.0:
```sql
ALTER TABLE embedding DROP COLUMN content_hash;
```

Otherwise, use the create-copy-drop-rename dance.

- [ ] **Step 2: Remove `content_hash` from `upsert_embedding` and `replace_embeddings_for_source`**

Remove the `content_hash` parameter and its bind from both functions.

- [ ] **Step 3: Remove `get_content_hash` function entirely**

Delete `get_content_hash` from `src/db/embeddings.rs`. Remove all call sites (should be
zero after Task 5 removed `reconcile_embeddings` and `embed_source`'s hash check).

- [ ] **Step 4: Simplify `embed_source`, `embed_source_forced`, and `embed_sources`**

Remove the hash-check logic from `embed_source`. It no longer checks
`get_content_hash` — if called, it always embeds. `embed_source_forced` can be collapsed
into `embed_source` since they now do the same thing (always embed). Same for
`embed_sources` (remove Phase 1 hash filtering).

The `content_hash` function in `pipeline.rs` is still used for `file_hash` computation,
so keep it (maybe rename to `file_hash` for clarity).

- [ ] **Step 5: Check if `upsert_embedding` has callers outside the pipeline**

Grep for `upsert_embedding`. If only called from tests, delete it entirely — the
codebase uses `replace_embeddings_for_source` for all real embedding writes.

- [ ] **Step 6: Update tests**

`tests/embeddings.rs` has tests for `get_content_hash` — remove those. Update any tests
that pass `content_hash` to `upsert_embedding` or `replace_embeddings_for_source`.
Remove `upsert_embedding` tests if the function was deleted.

- [ ] **Step 7: Run `just ci`**

- [ ] **Step 8: Commit**

```
refactor: remove content_hash from embedding table (superseded by file_hash)
```

---

## Chunk 4: Watcher Integration

### Task 7: Update real-time watcher to store `file_hash`

**Files:**
- Modify: `src/daemon/watcher.rs` — `process_batch` and `process_*_change` functions

The real-time watcher (triggered by filesystem events, not boot) also needs to compute
and store `file_hash` when a file changes. Since the watcher already reads the file
content, compute the hash and pass it to the CRUD functions.

- [ ] **Step 1: Update `process_batch` to compute hash and pass through**

In the watcher's `process_batch`, the file is read in `process_*_change`. Compute the
hash there and pass it to the DB write functions.

- [ ] **Step 2: Run `just ci`**

- [ ] **Step 3: Commit**

```
feat: store file_hash on real-time file change events
```

---

## Chunk 5: Tests and Verification

**Note:** Each prior task's `just ci` step includes fixing test compilation errors caused
by that task's changes (e.g. adding `file_hash: None` to test call sites, updating
struct constructions). Test fixes are incremental, not batched.

### Task 8: Add reconciliation performance test

**Files:**
- Modify: `tests/embeddings.rs` — add test for the new reconciliation path

- [ ] **Step 1: Write test for hash-skip behavior**

```rust
#[tokio::test]
async fn reconcile_filesystem_skips_unchanged_files() {
    // 1. Create workspace with a note file
    // 2. Run reconcile_filesystem → file processed, hash stored
    // 3. Run reconcile_filesystem again → file skipped (hash matches)
    // 4. Modify the file
    // 5. Run reconcile_filesystem → file re-processed, new hash stored
}
```

- [ ] **Step 2: Write test for missing-embeddings detection**

```rust
#[tokio::test]
async fn reconcile_filesystem_queues_embed_for_unembedded_files() {
    // 1. Create workspace with a note file
    // 2. Run reconcile_filesystem → file processed, hash stored
    // 3. Delete embeddings for that note (simulating Ollama-was-down)
    // 4. Run reconcile_filesystem → hash matches, but returns EmbedRequest
}
```

- [ ] **Step 3: Run `just ci`**

- [ ] **Step 4: Commit**

```
test: add reconciliation hash-skip and missing-embedding tests
```

---

### Task 9: Update sqlx offline data

**Files:**
- Modify: `.sqlx/` cached query data

- [ ] **Step 1: Regenerate sqlx offline data**

Run: `cargo sqlx prepare --workspace`

If using `query_as!` macros, this is needed. If using `query_as` (runtime), skip.

- [ ] **Step 2: Run `just ci` one final time**

- [ ] **Step 3: Commit**

```
chore: regenerate sqlx offline query data
```

---

## Summary of Changes

| What | Before | After |
|------|--------|-------|
| Boot reconciliation | Read + upsert every file, then re-hash-check every DB record for embeddings | Bulk load hashes, skip unchanged files entirely |
| Hash storage | `content_hash` on embedding chunks | `file_hash` on knowledge tables |
| File I/O | `std::fs::read_to_string` (blocking) | `tokio::fs::read_to_string` (async) |
| DB queries per boot | ~300+ (2-5 per file × 150 files) | 4 bulk SELECTs + writes only for changed files |
| Expected boot time | ~3s for 150 files | <200ms (steady state: ~50ms for hash comparison only) |
