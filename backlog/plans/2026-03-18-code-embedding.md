# Code Embedding for the Coding Agent — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or superpowers:executing-plans
> to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Automatically embed code from repos in `code/<slug>/` and make it queryable
through the existing `knowledge_search` tool.

**Architecture:** New `code_file` DB table + FTS5 + embeddings following the `script`
pattern. File watcher extended to watch `code/` with gitignore-aware walking (`ignore`
crate). `knowledge_search` gains `categories=["code"]` and a `repo` filter. Synthetic
topics (`"code/<slug>"`) reuse the existing vector search topic filtering. Coding agent
prompt updated with repo slug and search guidance.

**Tech Stack:** Rust, SQLite (sqlx), sqlite-vec, FTS5, `ignore` crate, tree-sitter,
Ollama embeddings.

**Spec:** `backlog/tasks/3-invisible-improvements/3-embed-code.md`

---

## File Structure

| Action | File                            | Responsibility                                          |
| ------ | ------------------------------- | ------------------------------------------------------- |
| Create | `migrations/013_code_files.sql` | code_file table + FTS5 + triggers                       |
| Modify | `src/db/knowledge/records.rs`   | `CodeFileRecord` struct                                 |
| Modify | `src/db/knowledge/crud.rs`      | code_file CRUD + hash loading                           |
| Modify | `src/db/knowledge/search.rs`    | `search_code_files()`                                   |
| Modify | `src/db/knowledge/mod.rs`       | re-export new functions/types                           |
| Modify | `src/embeddings/chunker.rs`     | add `.jsx` to `detect_code_language()`                  |
| Modify | `src/embeddings/pipeline.rs`    | gitignore-aware walk, code reconciliation, reverse-pass |
| Modify | `src/daemon/watcher.rs`         | watch `code/`, classify, process code changes           |
| Modify | `src/tools/knowledge_search.rs` | `"code"` category, `repo` param, dispatch               |
| Modify | `src/config_workspace.rs`       | add `"code"` to bootstrap dirs                          |
| Modify | `src/coding/prompt.rs`          | inject `repo_slug` template var                         |
| Modify | `prompts/coding-agent.md`       | code search + lib docs guidance                         |
| Modify | `Cargo.toml`                    | add `ignore` dependency                                 |

---

## Task 1: Migration + Data Model

**Files:**

- Create: `migrations/013_code_files.sql`
- Modify: `src/db/knowledge/records.rs`

- [ ] **Step 1: Write the migration**

Create `migrations/013_code_files.sql`:

```sql
CREATE TABLE code_file (
    id          TEXT PRIMARY KEY,
    repo        TEXT NOT NULL,
    path        TEXT NOT NULL,
    content     TEXT NOT NULL,
    file_hash   TEXT,
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
    updated_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
    UNIQUE(repo, path)
);

CREATE VIRTUAL TABLE code_file_fts USING fts5(
    repo, path, content,
    content=code_file, content_rowid=rowid,
    tokenize='porter unicode61'
);

-- Sync triggers (same pattern as script_fts in 008_scripts.sql)
CREATE TRIGGER code_file_ai AFTER INSERT ON code_file BEGIN
    INSERT INTO code_file_fts(rowid, repo, path, content)
    VALUES (new.rowid, new.repo, new.path, new.content);
END;

CREATE TRIGGER code_file_ad AFTER DELETE ON code_file BEGIN
    INSERT INTO code_file_fts(code_file_fts, rowid, repo, path, content)
    VALUES ('delete', old.rowid, old.repo, old.path, old.content);
END;

CREATE TRIGGER code_file_au AFTER UPDATE ON code_file BEGIN
    INSERT INTO code_file_fts(code_file_fts, rowid, repo, path, content)
    VALUES ('delete', old.rowid, old.repo, old.path, old.content);
    INSERT INTO code_file_fts(rowid, repo, path, content)
    VALUES (new.rowid, new.repo, new.path, new.content);
END;
```

Verify the trigger pattern matches `migrations/008_scripts.sql`.

- [ ] **Step 2: Add `CodeFileRecord` to records.rs**

Add to `src/db/knowledge/records.rs` (after `ScriptRecord`):

```rust
#[derive(Debug, Clone, Deserialize, Serialize, sqlx::FromRow)]
pub struct CodeFileRecord {
    pub id: String,
    pub repo: String,
    pub path: String,
    pub content: String,
    pub file_hash: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}
```

- [ ] **Step 3: Run `just ci` to verify migration applies and types compile**

Run: `just ci` Expected: PASS (migration applies, new struct compiles)

- [ ] **Step 4: Commit**

```
feat: add code_file table, FTS5, and CodeFileRecord
```

---

## Task 2: Code File CRUD

**Files:**

- Modify: `src/db/knowledge/crud.rs`
- Modify: `src/db/knowledge/mod.rs`

Follow the exact pattern of the script CRUD functions at `crud.rs:656-765`.

- [ ] **Step 1: Write CRUD functions**

Add to `src/db/knowledge/crud.rs` after the script section:

```rust
// ---------------------------------------------------------------------------
// Code Files
// ---------------------------------------------------------------------------

#[tracing::instrument(skip_all, level = "debug", fields(repo = %repo, path = %path))]
pub async fn create_code_file(
    db: &SqlitePool,
    repo: &str,
    path: &str,
    content: &str,
    file_hash: Option<&str>,
) -> Result<String, DatabaseError> {
    let id = new_id();
    let ts = now();
    sqlx::query(
        "INSERT INTO code_file (id, repo, path, content, file_hash, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(repo)
    .bind(path)
    .bind(content)
    .bind(file_hash)
    .bind(&ts)
    .bind(&ts)
    .execute(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "code_file",
        operation: "create",
        source,
    })?;
    Ok(id)
}

#[tracing::instrument(skip_all, level = "debug", fields(code_file_id = %code_file_id))]
pub async fn update_code_file(
    db: &SqlitePool,
    code_file_id: &str,
    content: &str,
    file_hash: Option<&str>,
) -> Result<(), DatabaseError> {
    sqlx::query(
        "UPDATE code_file SET content = ?, file_hash = ?, updated_at = ? WHERE id = ?",
    )
    .bind(content)
    .bind(file_hash)
    .bind(now())
    .bind(code_file_id)
    .execute(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "code_file",
        operation: "update",
        source,
    })?;
    Ok(())
}

pub async fn find_code_file(
    db: &SqlitePool,
    repo: &str,
    path: &str,
) -> Result<Option<CodeFileRecord>, DatabaseError> {
    sqlx::query_as::<_, CodeFileRecord>(
        "SELECT * FROM code_file WHERE repo = ? AND path = ? LIMIT 1",
    )
    .bind(repo)
    .bind(path)
    .fetch_optional(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "code_file",
        operation: "find",
        source,
    })
}

pub async fn delete_code_file(
    db: &SqlitePool,
    code_file_id: &str,
) -> Result<(), DatabaseError> {
    sqlx::query("DELETE FROM code_file WHERE id = ?")
        .bind(code_file_id)
        .execute(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "code_file",
            operation: "delete",
            source,
        })?;
    Ok(())
}

pub async fn delete_code_files_by_repo(
    db: &SqlitePool,
    repo: &str,
) -> Result<u64, DatabaseError> {
    let result = sqlx::query("DELETE FROM code_file WHERE repo = ?")
        .bind(repo)
        .execute(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "code_file",
            operation: "delete_by_repo",
            source,
        })?;
    Ok(result.rows_affected())
}
```

- [ ] **Step 2: Write hash loading function**

Add to `src/db/knowledge/crud.rs` (near `load_script_file_hashes` at line ~595). Returns
`(repo, path)` so the reconciler can build the full workspace-relative key:

```rust
pub async fn load_code_file_hashes(
    db: &SqlitePool,
) -> Result<Vec<CodeFileHashRecord>, DatabaseError> {
    sqlx::query_as::<_, CodeFileHashRecord>(
        "SELECT \
            cf.repo, cf.path, cf.file_hash, \
            (e.source_id IS NOT NULL) AS has_embeddings \
         FROM code_file cf \
         LEFT JOIN ( \
            SELECT DISTINCT source_id FROM embedding WHERE source_table = 'code_file' \
         ) e ON e.source_id = cf.id",
    )
    .fetch_all(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "code_file",
        operation: "load_file_hashes",
        source,
    })
}
```

And the record type (add near `FileHashRecord` in `crud.rs`):

```rust
#[derive(Debug, sqlx::FromRow)]
pub struct CodeFileHashRecord {
    pub repo: String,
    pub path: String,
    pub file_hash: Option<String>,
    pub has_embeddings: bool,
}
```

- [ ] **Step 3: Update mod.rs re-exports**

Add to `src/db/knowledge/mod.rs`:

- In the `pub use crud::{...}` block: add
  `CodeFileHashRecord, create_code_file, delete_code_file, delete_code_files_by_repo, find_code_file, load_code_file_hashes, update_code_file`
- In the `pub use records::{...}` block: add `CodeFileRecord`

- [ ] **Step 4: Run `just ci`**

Run: `just ci` Expected: PASS

- [ ] **Step 5: Commit**

```
feat: add code_file CRUD and hash loading functions
```

---

## Task 3: FTS5 Search Function

**Files:**

- Modify: `src/db/knowledge/search.rs`
- Modify: `src/db/knowledge/mod.rs`

Follow the pattern of `search_scripts()` at `search.rs:267-316`.

- [ ] **Step 1: Write `search_code_files()`**

Add to `src/db/knowledge/search.rs` after `search_scripts()`:

```rust
pub async fn search_code_files(
    db: &SqlitePool,
    query: &str,
    limit: usize,
    repo: Option<&str>,
) -> Result<Vec<SearchHit>, DatabaseError> {
    #[derive(sqlx::FromRow)]
    struct CodeSearchRow {
        id: String,
        repo: String,
        path: String,
        content: String,
        score: f64,
    }

    let fts_query = sanitize_fts_query(query);

    let rows = if let Some(repo_filter) = repo {
        sqlx::query_as::<_, CodeSearchRow>(
            "SELECT cf.id, cf.repo, cf.path, \
             snippet(code_file_fts, 2, '', '', '...', 80) AS content, \
             -bm25(code_file_fts, 1.0, 3.0, 1.0) AS score \
             FROM code_file_fts \
             JOIN code_file cf ON cf.rowid = code_file_fts.rowid \
             WHERE code_file_fts MATCH ? AND cf.repo = ? \
             ORDER BY score DESC \
             LIMIT ?",
        )
        .bind(&fts_query)
        .bind(repo_filter)
        .bind(limit as i64)
        .fetch_all(db)
        .await
    } else {
        sqlx::query_as::<_, CodeSearchRow>(
            "SELECT cf.id, cf.repo, cf.path, \
             snippet(code_file_fts, 2, '', '', '...', 80) AS content, \
             -bm25(code_file_fts, 1.0, 3.0, 1.0) AS score \
             FROM code_file_fts \
             JOIN code_file cf ON cf.rowid = code_file_fts.rowid \
             WHERE code_file_fts MATCH ? \
             ORDER BY score DESC \
             LIMIT ?",
        )
        .bind(&fts_query)
        .bind(limit as i64)
        .fetch_all(db)
        .await
    }
    .map_err(|source| DatabaseError::Query {
        table: "code_file",
        operation: "search",
        source,
    })?;

    Ok(rows
        .into_iter()
        .map(|r| {
            let snippet = truncate_snippet(&r.content, 500);
            SearchHit {
                id: r.id,
                title: format!("{}/{}", r.repo, r.path),
                snippet,
                score: r.score,
                kind: "code".to_string(),
                path: Some(format!("code/{}/{}", r.repo, r.path)),
            }
        })
        .collect())
}
```

Note: `snippet()` column index 2 = `content` (0=repo, 1=path, 2=content). BM25 weights
`(1.0, 3.0, 1.0)` = repo baseline, path 3x, content 1x.

- [ ] **Step 2: Update mod.rs re-exports**

Add `search_code_files` to the `search` use block in `src/db/knowledge/mod.rs`.

- [ ] **Step 3: Run `just ci`**

Run: `just ci` Expected: PASS

- [ ] **Step 4: Commit**

```
feat: add search_code_files FTS5 search with repo filtering
```

---

## Task 4: knowledge_search Tool Integration

**Files:**

- Modify: `src/tools/knowledge_search.rs`

- [ ] **Step 1: Add `"code"` to categories enum and `repo` parameter**

In `schema()` (around line 47), update the enum:

```rust
"enum": ["notes", "references", "diary", "topics", "scripts", "code"]
```

Update the description:

```rust
"description": "Categories to search. Defaults to [\"notes\", \"diary\"]. \
    Include \"references\" to search reference material, \"scripts\" for scripts, \
    \"code\" for indexed repository code. Use \"topics\" to search topic collections."
```

Add `repo` parameter to the `properties` object (after `topic`):

```rust
"repo": {
    "type": "string",
    "description": "Scope code search to a repo slug \
        (e.g. \"ghost\"). Prefix matching. \
        Only affects code category and vector search. \
        Auto-includes \"code\" category when set."
}
```

- [ ] **Step 2: Add dispatch logic**

After `search_scripts_flag` (line ~102), add:

```rust
let repo = params.get("repo").and_then(Value::as_str);
let search_code_flag =
    repo.is_some() || (!use_defaults && categories.iter().any(|c| c == "code"));
```

After the scripts BM25 dispatch (line ~167), add:

```rust
if search_code_flag {
    bm25_hits.extend(
        search_code_files(&ctx.db, query, limit, repo)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?,
    );
}
```

Add `search_code_files` to the import at the top of the file.

- [ ] **Step 3: Add to effective_categories and filter_embedding_hits**

In the `effective_categories` block (line ~184), add:

```rust
if search_code_flag {
    effective_categories.push("code".to_string());
}
```

In `filter_embedding_hits()` (line ~284), add the mapping:

```rust
"code" => "code_file",
```

- [ ] **Step 4: Resolve `repo` to topic IDs for vector search**

The `resolved_topic_ids` are already passed to `try_hybrid_search` which passes them to
`vector_search`. We need to merge repo topic IDs into this list.

After `resolved_topic_ids` construction (line ~112), add:

```rust
let repo_topic_ids = if let Some(repo_name) = repo {
    let code_topic = format!("code/{repo_name}");
    let topics = find_topics_by_prefix(&ctx.db, &code_topic)
        .await
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
    topics.into_iter().map(|t| t.id).collect::<Vec<_>>()
} else {
    vec![]
};
```

Then merge both sets before passing to `try_hybrid_search`. Replace the
`resolved_topic_ids` reference with a combined vector:

```rust
let mut all_topic_ids = resolved_topic_ids;
all_topic_ids.extend(repo_topic_ids);
```

Pass `&all_topic_ids` to `try_hybrid_search`.

- [ ] **Step 5: Add `"Code"` to format_results kind_header**

In `format_results()` (line ~324), add:

```rust
"code" => "Code",
```

- [ ] **Step 6: Run `just ci`**

Run: `just ci` Expected: PASS

- [ ] **Step 7: Commit**

```
feat: add code category and repo filter to knowledge_search tool
```

---

## Task 5: JSX Chunker Support

**Files:**

- Modify: `src/embeddings/chunker.rs`

- [ ] **Step 1: Add `.jsx` to `detect_code_language()`**

In `detect_code_language()` at `chunker.rs:344-361`, add `"jsx"` to the JavaScript match
arm:

```rust
"js" | "mjs" | "cjs" | "jsx" => Some((tree_sitter_javascript::LANGUAGE.into(), "javascript")),
```

- [ ] **Step 2: Run `just ci`**

Run: `just ci` Expected: PASS

- [ ] **Step 3: Commit**

```
feat: add JSX support to code chunker
```

---

## Task 6: Add `ignore` Dependency + Code File Utilities

**Files:**

- Modify: `Cargo.toml`
- Modify: `src/embeddings/pipeline.rs` (add helper functions)

- [ ] **Step 1: Add `ignore` crate to Cargo.toml**

```
ignore = "0.4"
```

- [ ] **Step 2: Add code-specific walking utilities to pipeline.rs**

Add at the bottom of `src/embeddings/pipeline.rs` (before tests if any):

```rust
/// File extensions eligible for code embedding.
pub(crate) const CODE_EXTENSIONS: &[&str] = &[
    // Tree-sitter supported (AST-aware chunking)
    "rs", "py", "js", "jsx", "ts", "tsx", "go", "sh", "bash", "toml", "json",
    // Text fallback (line-based chunking)
    "c", "h", "cpp", "hpp", "java", "kt", "rb", "sql", "lua", "zig",
    "ex", "exs", "yaml", "yml", "md",
];

/// Maximum file size for code embedding (100KB).
pub(crate) const MAX_CODE_FILE_SIZE: u64 = 100 * 1024;

/// Walk a repo directory respecting .gitignore, extension allowlist, and size limit.
pub(crate) fn walk_code_repo(repo_dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    let walker = ignore::WalkBuilder::new(repo_dir)
        .hidden(true)       // skip hidden files (but .gitignore still read)
        .git_ignore(true)   // respect .gitignore
        .git_exclude(true)  // respect .git/info/exclude
        .build();

    for entry in walker.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        // Extension allowlist
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !CODE_EXTENSIONS.contains(&ext) {
            continue;
        }
        // Size guard
        if let Ok(meta) = path.metadata() {
            if meta.len() > MAX_CODE_FILE_SIZE {
                tracing::debug!(
                    path = path.display().to_string(),
                    size = meta.len(),
                    "skipping large code file"
                );
                continue;
            }
        }
        files.push(path.to_path_buf());
    }
    files
}

/// Extract repo slug from a path under `code/`. Returns `None` if not a code path.
pub(crate) fn extract_repo_slug(workspace: &std::path::Path, path: &std::path::Path) -> Option<String> {
    let rel = path.strip_prefix(workspace).ok()?;
    let mut components = rel.components();
    let first = components.next()?;
    if first.as_os_str() != "code" {
        return None;
    }
    let slug = components.next()?;
    Some(slug.as_os_str().to_string_lossy().to_string())
}
```

- [ ] **Step 3: Run `just ci`**

Run: `just ci` Expected: PASS (new functions unused for now — clippy may warn, add
`#[allow(dead_code)]` temporarily or proceed to Task 7 before committing)

- [ ] **Step 4: Commit**

```
feat: add ignore crate and code-specific walking utilities
```

---

## Task 7: Workspace Bootstrap + Watcher

**Files:**

- Modify: `src/config_workspace.rs`
- Modify: `src/daemon/watcher.rs`

- [ ] **Step 1: Add `"code"` to workspace bootstrap**

In `src/config_workspace.rs`, add `"code"` to the directory list at line ~24:

```rust
for dir in [
    "skills",
    "agents",
    ".cache",
    "notes",
    "references",
    "diary",
    "projects",
    "shell",
    "feedback",
    "scripts",
    "code",       // <-- add this
] {
```

- [ ] **Step 2: Add `code/` to watcher directories**

In `src/daemon/watcher.rs`, around line 162, add:

```rust
let code_dir = workspace.join("code");
```

Add `&code_dir` to the watch loop at line ~164:

```rust
for dir in [&notes_dir, &refs_dir, &diary_dir, &scripts_dir, &code_dir] {
```

- [ ] **Step 3: Add `"code"` to `classify_watcher_kind()`**

In `classify_watcher_kind()` at line ~184, add before the `"unknown"` fallback:

```rust
} else if rel.starts_with("code/") {
    "code"
```

- [ ] **Step 4: Add `process_code_file_change()` and dispatch**

In `process_change()` at line ~244, add before the `else` fallback:

```rust
} else if rel_str.starts_with("code/") {
    process_code_file_change(db, workspace, path, raw_content, file_hash).await
```

Then add the handler function (model after `process_script_change` at line ~547):

```rust
/// Sync a changed code file to the database.
///
/// Code files are indexed from repos in `code/<slug>/`. The repo slug is
/// derived from the first directory component after `code/`. Files must pass
/// the extension allowlist and size guard (enforced by caller/reconciler).
async fn process_code_file_change(
    db: &GhostDb,
    workspace: &Path,
    path: &Path,
    raw_content: Option<&str>,
    file_hash: Option<&str>,
) -> Result<Option<EmbedRequest>, PipelineError> {
    let repo = match crate::embeddings::pipeline::extract_repo_slug(workspace, path) {
        Some(r) => r,
        None => return Ok(None),
    };

    let fs_rel = path
        .strip_prefix(workspace)
        .unwrap_or(path)
        .to_string_lossy()
        .to_string();

    // DB stores path relative to repo root (strip "code/<slug>/")
    let code_path = fs_rel
        .strip_prefix(&format!("code/{repo}/"))
        .unwrap_or(&fs_rel)
        .to_string();

    // Extension check
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    if !crate::embeddings::pipeline::CODE_EXTENSIONS.contains(&ext) {
        return Ok(None);
    }

    // Size guard
    if let Ok(meta) = path.metadata() {
        if meta.len() > crate::embeddings::pipeline::MAX_CODE_FILE_SIZE {
            return Ok(None);
        }
    }

    // Deletion
    if !path.exists() {
        if let Ok(Some(cf)) = crate::db::knowledge::find_code_file(db, &repo, &code_path).await {
            crate::db::embeddings::delete_embeddings_for_source(db, &cf.id).await?;
            crate::db::knowledge::delete_code_file(db, &cf.id).await?;
            logfire::info!("watcher: deleted code file", repo = repo, path = code_path);
        }
        return Ok(None);
    }

    let owned;
    let content = match raw_content {
        Some(c) => c,
        None => {
            owned = match tokio::fs::read_to_string(path).await {
                Ok(c) => c,
                Err(_) => return Ok(None),
            };
            &owned
        }
    };

    // Find or create synthetic topic for this repo
    let topic_name = format!("code/{repo}");
    let topic_id = crate::db::knowledge::find_or_create_topic(db, &topic_name).await?;

    let code_file_id =
        match crate::db::knowledge::find_code_file(db, &repo, &code_path).await {
            Ok(Some(cf)) => {
                let _ =
                    crate::db::knowledge::update_code_file(db, &cf.id, content, file_hash).await;
                cf.id
            }
            _ => {
                match crate::db::knowledge::create_code_file(
                    db, &repo, &code_path, content, file_hash,
                )
                .await
                {
                    Ok(id) => id,
                    Err(_) => return Ok(None),
                }
            }
        };

    Ok(Some(EmbedRequest {
        source_table: "code_file".into(),
        source_id: code_file_id,
        content: content.to_string(),
        tags: vec![],
        topic_id: Some(topic_id),
        path: Some(fs_rel),
    }))
}
```

Note: `extract_repo_slug`, `CODE_EXTENSIONS`, `MAX_CODE_FILE_SIZE`, and `walk_code_repo`
are declared `pub(crate)` in `pipeline.rs` (Task 6). Error propagation uses `?` operator
— `PipelineError` has `#[from] DatabaseError` so conversions are automatic.

- [ ] **Step 5: Run `just ci`**

Run: `just ci` Expected: PASS

- [ ] **Step 6: Commit**

```
feat: watch code/ directory and sync code files to DB
```

---

## Task 8: Reconciliation — Code Phase + Reverse-Pass

**Files:**

- Modify: `src/embeddings/pipeline.rs`

- [ ] **Step 1: Add code file hashes to `reconcile_filesystem()` Phase 1**

In `reconcile_filesystem()` at line ~218, after loading script hashes, add:

```rust
let code_hashes = db::knowledge::load_code_file_hashes(db).await?;
```

Build the `known` map entries (after script hash insertion at line ~240):

```rust
for r in &code_hashes {
    known.insert(
        format!("code/{}/{}", r.repo, r.path),
        (r.file_hash.clone(), r.has_embeddings),
    );
}
```

- [ ] **Step 2: Add code directory walking to Phase 2**

The existing Phase 2 walks `["notes", "references", "diary", "scripts"]`. Code needs a
separate loop because it uses `walk_code_repo()` (gitignore-aware) instead of the naive
`walk_directory()`.

After the existing `for subdir in [...]` loop (after line ~302), add:

```rust
// Phase 2b: Walk code repos (gitignore-aware)
let code_dir = workspace.join("code");
if code_dir.exists() {
    if let Ok(entries) = std::fs::read_dir(&code_dir) {
        for entry in entries.flatten() {
            if !entry.path().is_dir() {
                continue;
            }
            let repo_dir = entry.path();
            let files = walk_code_repo(&repo_dir);
            for file_path in files {
                let rel = file_path.strip_prefix(workspace).unwrap_or(&file_path);
                let rel_str = rel.to_string_lossy().to_string();

                let raw = match tokio::fs::read_to_string(&file_path).await {
                    Ok(r) => r,
                    Err(_) => continue,
                };
                let hash = content_hash(&raw);

                match known.get(&rel_str) {
                    Some((Some(stored_hash), true)) if *stored_hash == hash => continue,
                    Some((Some(stored_hash), false)) if *stored_hash == hash => {
                        if let Some(req) =
                            build_embed_request_from_db(db, workspace, &file_path, &raw).await
                        {
                            embed_requests.push(req);
                        }
                    }
                    _ => {
                        match crate::daemon::watcher::process_change(
                            db, workspace, &file_path, Some(&raw), Some(&hash),
                        )
                        .await
                        {
                            Ok(Some(req)) => {
                                embed_requests.push(req);
                                discovered += 1;
                            }
                            Ok(None) => {}
                            Err(e) => {
                                tracing::warn!(
                                    path = file_path.display().to_string(),
                                    error = e.to_string(),
                                    "reconcile: failed to process code file"
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}
```

Also track which code paths were seen on disk for the reverse-pass:

Add `let mut seen_code_paths: HashSet<String> = HashSet::new();` before the code loop,
and insert `seen_code_paths.insert(rel_str.clone());` inside the loop (before the
`match known.get`).

- [ ] **Step 3: Add reverse-pass for stale code file cleanup**

After Phase 2b, add:

```rust
// Phase 3: Reverse-pass — delete stale code_file records
for r in &code_hashes {
    let rel_key = format!("code/{}/{}", r.repo, r.path);
    if !seen_code_paths.contains(&rel_key) {
        // File no longer exists on disk — clean up
        if let Ok(Some(cf)) =
            db::knowledge::find_code_file(db, &r.repo, &r.path).await
        {
            if let Err(e) = db::embeddings::delete_embeddings_for_source(db, &cf.id).await {
                tracing::warn!(
                    repo = r.repo,
                    path = r.path,
                    error = e.to_string(),
                    "reconcile: failed to delete code file embeddings"
                );
            }
            if let Err(e) = db::knowledge::delete_code_file(db, &cf.id).await {
                tracing::warn!(
                    repo = r.repo,
                    path = r.path,
                    error = e.to_string(),
                    "reconcile: failed to delete stale code file"
                );
            } else {
                tracing::info!(
                    repo = r.repo,
                    path = r.path,
                    "reconcile: removed stale code file"
                );
            }
        }
    }
}
```

- [ ] **Step 4: Add `code/` to `build_embed_request_from_db()`**

In `build_embed_request_from_db()` at line ~368, before the `else { None }` fallback,
add:

```rust
} else if rel_str.starts_with("code/") {
    let slug = extract_repo_slug(workspace, file_path)?;
    let code_path = rel_str
        .strip_prefix(&format!("code/{slug}/"))
        .unwrap_or(&rel_str);
    let cf = db::knowledge::find_code_file(db, &slug, code_path)
        .await
        .ok()??;
    let topic_name = format!("code/{slug}");
    let topic_id = db::knowledge::find_or_create_topic(db, &topic_name)
        .await
        .ok()?;
    Some(EmbedRequest {
        source_table: "code_file".into(),
        source_id: cf.id,
        content: content.to_string(),
        tags: vec![],
        topic_id: Some(topic_id),
        path: Some(rel_str.to_string()),
    })
```

- [ ] **Step 5: Run `just ci`**

Run: `just ci` Expected: PASS

- [ ] **Step 6: Commit**

```
feat: add code repo reconciliation with gitignore walk and reverse-pass cleanup
```

---

## Task 9: Coding Agent Prompt

**Files:**

- Modify: `src/coding/prompt.rs`
- Modify: `prompts/coding-agent.md`

- [ ] **Step 1: Inject `repo_slug` in `build_coding_prompt()`**

In `src/coding/prompt.rs`, in `build_coding_prompt()` (line ~17), add:

```rust
let repo_slug = working_dir
    .file_name()
    .map(|n| n.to_string_lossy().to_string())
    .unwrap_or_default();
vars.insert("repo_slug", repo_slug);
```

- [ ] **Step 2: Add code search and lib docs sections to coding-agent.md**

Add after the `## Working Directory` section (before `## Workflow`):

```markdown
## Code Search

Your repo (`{{ repo_slug }}`) is indexed and searchable. Use `knowledge_search` to find
relevant code:

- Search this repo: `knowledge_search(categories=["code"], repo="{{ repo_slug }}")`
- Search all indexed repos: `knowledge_search(categories=["code"])`
- Search code + library docs together:
  `knowledge_search(categories=["code", "references"], repo="{{ repo_slug }}")`

## Library Documentation

Check what libraries/frameworks the repo uses (look at `Cargo.toml`, `package.json`,
`pyproject.toml`, `go.mod`, etc.) and search for existing reference docs:
```

knowledge_search(categories=["references"], topic="<library-name>")

```

If docs aren't imported yet, use the shell to import them:

```

ghost reference import git --url <docs-repo-url> --topic <library-name> --extensions md
ghost reference import crawl --url <docs-url> --topic <library-name>

```

```

- [ ] **Step 3: Run `just ci`**

Run: `just ci` Expected: PASS

- [ ] **Step 4: Commit**

```
feat: add code search and lib docs guidance to coding agent prompt
```

---

## Task 10: Integration Testing

**Files:**

- Tests in existing test files or new test module

- [ ] **Step 1: Write a unit test for code file CRUD**

Add to the test module in `src/db/knowledge/crud.rs` (or wherever script tests live):

```rust
#[tokio::test]
async fn code_file_crud_roundtrip() {
    let db = test_db().await;

    let id = create_code_file(&db, "ghost", "src/main.rs", "fn main() {}", Some("abc123"))
        .await
        .unwrap();

    let found = find_code_file(&db, "ghost", "src/main.rs").await.unwrap().unwrap();
    assert_eq!(found.id, id);
    assert_eq!(found.repo, "ghost");
    assert_eq!(found.content, "fn main() {}");

    update_code_file(&db, &id, "fn main() { todo!() }", Some("def456"))
        .await
        .unwrap();
    let updated = find_code_file(&db, "ghost", "src/main.rs").await.unwrap().unwrap();
    assert_eq!(updated.content, "fn main() { todo!() }");

    delete_code_file(&db, &id).await.unwrap();
    let gone = find_code_file(&db, "ghost", "src/main.rs").await.unwrap();
    assert!(gone.is_none());
}
```

Use the project's existing test DB helper (`test_db()` or `test_config()` + connect).
Read the `/testing` skill for the exact helper signatures.

- [ ] **Step 2: Write a unit test for `search_code_files`**

```rust
#[tokio::test]
async fn search_code_files_filters_by_repo() {
    let db = test_db().await;

    create_code_file(&db, "ghost", "src/main.rs", "fn main() { start_ghost() }", Some("a"))
        .await
        .unwrap();
    create_code_file(&db, "other", "src/lib.rs", "fn start_ghost() {}", Some("b"))
        .await
        .unwrap();

    // Search with repo filter — only ghost results
    let hits = search_code_files(&db, "ghost", 10, Some("ghost")).await.unwrap();
    assert!(hits.iter().all(|h| h.path.as_deref().unwrap().starts_with("code/ghost/")));

    // Search without filter — both repos
    let all = search_code_files(&db, "ghost", 10, None).await.unwrap();
    assert!(all.len() >= 2, "should find results from both repos");
}
```

- [ ] **Step 3: Write a test for `extract_repo_slug`**

```rust
#[test]
fn extract_repo_slug_from_code_path() {
    let ws = std::path::Path::new("/home/user/GHOST");
    let path = std::path::Path::new("/home/user/GHOST/code/myapp/src/main.rs");
    assert_eq!(extract_repo_slug(ws, path), Some("myapp".to_string()));

    let non_code = std::path::Path::new("/home/user/GHOST/notes/foo.md");
    assert_eq!(extract_repo_slug(ws, non_code), None);
}
```

- [ ] **Step 4: Write a test for `walk_code_repo` gitignore filtering**

```rust
#[test]
fn walk_code_repo_respects_gitignore() {
    let dir = tempfile::TempDir::new().unwrap();
    let repo = dir.path();

    // Create a minimal git repo with .gitignore
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    std::fs::write(repo.join(".gitignore"), "target/\n*.log\n").unwrap();
    std::fs::create_dir_all(repo.join("src")).unwrap();
    std::fs::write(repo.join("src/main.rs"), "fn main() {}").unwrap();
    std::fs::create_dir_all(repo.join("target")).unwrap();
    std::fs::write(repo.join("target/debug.rs"), "ignored").unwrap();
    std::fs::write(repo.join("build.log"), "ignored").unwrap();

    let files = walk_code_repo(repo);
    let names: Vec<&str> = files.iter().filter_map(|p| p.file_name()?.to_str()).collect();

    assert!(names.contains(&"main.rs"));
    assert!(!names.contains(&"debug.rs"), "target/ should be gitignored");
    assert!(!names.contains(&"build.log"), "*.log should be gitignored");
}
```

- [ ] **Step 5: Run all tests**

Run: `just ci` Expected: All tests PASS

- [ ] **Step 6: Commit**

```
test: add code file CRUD, search, walk, and slug extraction tests
```

---

## Task 11: Final Verification

- [ ] **Step 1: Run `just ci` one final time**

Run: `just ci` Expected: PASS — fmt, check, clippy, all tests green

- [ ] **Step 2: Manual smoke test**

1. Start ghost daemon
2. Clone a small repo into `~/GHOST/code/test-repo/`
3. Wait a few seconds for the watcher to pick up files
4. Check DB:
   `ghost shell command="sqlite3 ~/GHOST/ghost.db 'SELECT repo, path FROM code_file LIMIT 5'"`
5. Test search: send a message using the coding agent that triggers
   `knowledge_search(categories=["code"], repo="test-repo")`

- [ ] **Step 3: Commit any final fixes**

```
chore: final polish for code embedding feature
```
