# Custom Scripts Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if
> subagents available) or superpowers:executing-plans to implement this plan. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let the GHOST write, index, and reuse scripts as a first-class knowledge
category with a scripting skill that teaches opinionated coding guidelines.

**Architecture:** New `script` DB table + FTS5 + embedding pipeline integration (mirrors
notes/references/diary pattern). Filesystem watcher extended to watch `scripts/`.
`knowledge_search` tool gains `"scripts"` category. Bundled workspace skill teaches the
GHOST how to write scripts. E2e tests verify end-to-end behavior.

**Tech Stack:** SQLite (sqlx), FTS5, sqlite-vec, tree-sitter (Python/Bash chunking),
Ollama (qwen3-embedding:8b), uv (PEP 723 scripts), typer (CLI args).

**Spec:** `specs/done/custom_scripts.md`

---

## Chunk 1: Database + CRUD Layer

### Task 1: Migration — `script` table + FTS5 + triggers

**Files:**

- Create: `migrations/NNN_scripts.sql` (use next sequence number after existing)

- [ ] **Step 1: Check existing migration numbering**

Run: `ls migrations/`

Determine the next migration number. Currently `001_initial.sql` through
`007_fix_reference_fts.sql` exist, so the next is `008`.

- [ ] **Step 2: Write the migration**

Create `migrations/008_scripts.sql`:

```sql
-- Script knowledge type: executable artifacts the GHOST writes and reuses.

CREATE TABLE script (
    id         TEXT PRIMARY KEY NOT NULL,
    path       TEXT NOT NULL UNIQUE,  -- relative to workspace: scripts/finance/spending.py
    content    TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE VIRTUAL TABLE script_fts USING fts5(
    path,
    content,
    content=script,
    content_rowid=rowid,
    tokenize='porter unicode61'
);

CREATE TRIGGER script_fts_ai AFTER INSERT ON script BEGIN
    INSERT INTO script_fts(rowid, path, content)
    VALUES (new.rowid, new.path, new.content);
END;

CREATE TRIGGER script_fts_ad AFTER DELETE ON script BEGIN
    INSERT INTO script_fts(script_fts, rowid, path, content)
    VALUES ('delete', old.rowid, old.path, old.content);
END;

CREATE TRIGGER script_fts_au AFTER UPDATE ON script BEGIN
    INSERT INTO script_fts(script_fts, rowid, path, content)
    VALUES ('delete', old.rowid, old.path, old.content);
    INSERT INTO script_fts(rowid, path, content)
    VALUES (new.rowid, new.path, new.content);
END;
```

- [ ] **Step 3: Verify migration applies cleanly**

Run: `cargo test --lib -- db 2>&1 | tail -20`

The test suite will attempt to apply migrations. If compilation succeeds and DB tests
pass, the migration is valid.

- [ ] **Step 4: Commit**

```bash
git add migrations/008_scripts.sql
git commit -m "feat: add script table, FTS5 index, and sync triggers"
```

---

### Task 2: ScriptRecord type

**Files:**

- Modify: `src/db/knowledge/records.rs` (add `ScriptRecord`)

- [ ] **Step 1: Add ScriptRecord struct**

Add to `src/db/knowledge/records.rs`, after the existing record types:

```rust
#[derive(Debug, Clone, Deserialize, Serialize, sqlx::FromRow)]
pub struct ScriptRecord {
    pub id: String,
    pub path: String,
    pub content: String,
    pub created_at: String,
    pub updated_at: String,
}
```

- [ ] **Step 2: Export from `records.rs`**

The struct is already public. Update `src/db/knowledge/mod.rs` to add `ScriptRecord` to
the `pub use records::` line:

```rust
pub use records::{
    DiaryRecord, EdgeRecord, ImportBatchRecord, NoteRecord, RecentItem, ReferenceRecord,
    ScriptRecord, SearchHit, TopicRecord,
};
```

- [ ] **Step 3: Verify compilation**

Run: `cargo check`

- [ ] **Step 4: Commit**

```bash
git add src/db/knowledge/records.rs src/db/knowledge/mod.rs
git commit -m "feat: add ScriptRecord type"
```

---

### Task 3: Script CRUD functions

**Files:**

- Modify: `src/db/knowledge/crud.rs` (add script CRUD)
- Modify: `src/db/knowledge/mod.rs` (export new functions)

- [ ] **Step 1: Add CRUD functions to `crud.rs`**

Add at the end of `src/db/knowledge/crud.rs` (before the closing of the file), following
the exact patterns used for notes/references:

```rust
// ---------------------------------------------------------------------------
// Scripts
// ---------------------------------------------------------------------------

#[tracing::instrument(skip_all, level = "debug", fields(path = %path))]
pub async fn create_script(
    db: &SqlitePool,
    path: &str,
    content: &str,
) -> Result<String, DatabaseError> {
    let id = new_id();
    let ts = now();

    sqlx::query(
        "INSERT INTO script (id, path, content, created_at, updated_at) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(path)
    .bind(content)
    .bind(&ts)
    .bind(&ts)
    .execute(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "script",
        operation: "create",
        source,
    })?;

    Ok(id)
}

#[tracing::instrument(skip_all, level = "debug", fields(script_id = %script_id))]
pub async fn update_script(
    db: &SqlitePool,
    script_id: &str,
    content: &str,
) -> Result<(), DatabaseError> {
    sqlx::query("UPDATE script SET content = ?, updated_at = ? WHERE id = ?")
        .bind(content)
        .bind(now())
        .bind(script_id)
        .execute(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "script",
            operation: "update",
            source,
        })?;
    Ok(())
}

#[tracing::instrument(skip_all, level = "debug", fields(script_id = %script_id))]
pub async fn get_script(
    db: &SqlitePool,
    script_id: &str,
) -> Result<ScriptRecord, DatabaseError> {
    sqlx::query_as::<_, ScriptRecord>("SELECT * FROM script WHERE id = ?")
        .bind(script_id)
        .fetch_optional(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "script",
            operation: "get",
            source,
        })?
        .ok_or(DatabaseError::MissingRow {
            table: "script",
            operation: "get",
        })
}

#[tracing::instrument(skip_all, level = "debug", fields(path = %path))]
pub async fn find_script_by_path(
    db: &SqlitePool,
    path: &str,
) -> Result<Option<ScriptRecord>, DatabaseError> {
    sqlx::query_as::<_, ScriptRecord>("SELECT * FROM script WHERE path = ? LIMIT 1")
        .bind(path)
        .fetch_optional(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "script",
            operation: "find_by_path",
            source,
        })
}

#[tracing::instrument(skip_all, level = "debug", fields(script_id = %script_id))]
pub async fn delete_script(
    db: &SqlitePool,
    script_id: &str,
) -> Result<(), DatabaseError> {
    sqlx::query("DELETE FROM script WHERE id = ?")
        .bind(script_id)
        .execute(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "script",
            operation: "delete",
            source,
        })?;
    Ok(())
}

pub async fn list_all_scripts(
    db: &SqlitePool,
) -> Result<Vec<ScriptRecord>, DatabaseError> {
    sqlx::query_as::<_, ScriptRecord>("SELECT * FROM script")
        .fetch_all(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "script",
            operation: "list_all",
            source,
        })
}

pub async fn list_scripts_page(
    db: &SqlitePool,
    offset: usize,
    limit: usize,
) -> Result<Vec<ScriptRecord>, DatabaseError> {
    sqlx::query_as::<_, ScriptRecord>(
        "SELECT * FROM script ORDER BY id LIMIT ? OFFSET ?",
    )
    .bind(limit as i64)
    .bind(offset as i64)
    .fetch_all(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "script",
        operation: "list_page",
        source,
    })
}
```

- [ ] **Step 2: Add `ScriptRecord` import to `crud.rs`**

At the top of `crud.rs`, add `ScriptRecord` to the `use super::records::` import.

- [ ] **Step 3: Export from `mod.rs`**

Update `src/db/knowledge/mod.rs` to export the new functions:

```rust
pub use crud::{
    // ... existing exports ...,
    create_script, delete_script, find_script_by_path, get_script, list_all_scripts,
    list_scripts_page, update_script,
};
```

- [ ] **Step 4: Verify compilation**

Run: `cargo check`

- [ ] **Step 5: Commit**

```bash
git add src/db/knowledge/crud.rs src/db/knowledge/mod.rs
git commit -m "feat: script CRUD functions"
```

---

### Task 4: Script BM25 search + stats

**Files:**

- Modify: `src/db/knowledge/search.rs` (add `search_scripts`)
- Modify: `src/db/knowledge/stats.rs` (add `count_scripts`)
- Modify: `src/db/knowledge/mod.rs` (export)

- [ ] **Step 1: Add `search_scripts` to `search.rs`**

Follow the `search_notes` pattern exactly. Add after the existing search functions:

```rust
#[tracing::instrument(skip_all, level = "debug", fields(query = %query))]
pub async fn search_scripts(
    db: &SqlitePool,
    query: &str,
    limit: usize,
) -> Result<Vec<SearchHit>, DatabaseError> {
    #[derive(sqlx::FromRow)]
    struct ScriptSearchRow {
        id: String,
        path: String,
        content: String,
        score: f64,
    }

    let fts_query = sanitize_fts_query(query);

    let rows = sqlx::query_as::<_, ScriptSearchRow>(
        "SELECT s.id, s.path, \
         snippet(script_fts, 1, '', '', '...', 80) AS content, \
         -bm25(script_fts, 1.0, 1.0) AS score \
         FROM script_fts \
         JOIN script s ON s.rowid = script_fts.rowid \
         WHERE script_fts MATCH ? \
         ORDER BY score DESC \
         LIMIT ?",
    )
    .bind(&fts_query)
    .bind(limit as i64)
    .fetch_all(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "script",
        operation: "search",
        source,
    })?;

    Ok(rows
        .into_iter()
        .map(|r| {
            let snippet = truncate_snippet(&r.content, 500);
            SearchHit {
                id: r.id,
                title: r.path.clone(),
                snippet,
                score: r.score,
                kind: "script".to_string(),
                path: Some(format!("scripts/{}", r.path)),
            }
        })
        .collect())
}
```

Note: BM25 weights are `1.0, 1.0` (path and content equally weighted). The `title` field
of `SearchHit` is set to the script path since scripts don't have separate titles.

- [ ] **Step 2: Add `count_scripts` to `stats.rs`**

```rust
pub async fn count_scripts(db: &SqlitePool) -> Result<i64, DatabaseError> {
    count_table(db, "script").await
}
```

- [ ] **Step 3: Export from `mod.rs`**

Add to the `search` and `stats` pub use lines:

```rust
pub use search::{hybrid_merge, search_diary, search_notes, search_references, search_scripts, search_topics};
pub use stats::{
    count_diary, count_edges, count_notes, count_references, count_scripts, count_stubs,
    list_tags_with_counts,
};
```

- [ ] **Step 4: Verify compilation**

Run: `cargo check`

- [ ] **Step 5: Commit**

```bash
git add src/db/knowledge/search.rs src/db/knowledge/stats.rs src/db/knowledge/mod.rs
git commit -m "feat: script BM25 search and count"
```

---

## Chunk 2: Bootstrap + Watcher + Embedding Pipeline + Search Integration

### Task 5: Create `scripts/` directory on workspace bootstrap

**Files:**

- Modify: `src/config_workspace.rs`

This must be done BEFORE the watcher task — `setup_watcher` only watches directories
that exist.

- [ ] **Step 1: Check existing bootstrap logic**

Read `src/config_workspace.rs` and find where directories like `notes/`, `references/`,
`diary/` are created.

- [ ] **Step 2: Add `scripts/` directory creation**

In the same block that creates `notes/`, `references/`, `diary/`, add:

```rust
let scripts_dir = workspace.join("scripts");
if !scripts_dir.exists() {
    std::fs::create_dir_all(&scripts_dir)?;
}
```

- [ ] **Step 3: Verify compilation**

Run: `cargo check`

- [ ] **Step 4: Commit**

```bash
git add src/config_workspace.rs
git commit -m "feat: create scripts/ directory on workspace bootstrap"
```

---

### Task 6: Filesystem watcher — script support

**Files:**

- Modify: `src/daemon/watcher.rs`

Three changes needed in this file:

1. `setup_watcher` — watch `scripts/` directory
2. `classify_watcher_kind` — classify `scripts/` paths
3. `process_change` — route to new `process_script_change`
4. New `process_script_change` function

- [ ] **Step 1: Add `scripts/` to `setup_watcher`**

In `setup_watcher`, after the existing `notes_dir`, `refs_dir`, `diary_dir` lines, add:

```rust
let scripts_dir = workspace.join("scripts");
```

And add `&scripts_dir` to the `for dir in [...]` array.

- [ ] **Step 2: Add `"script"` to `classify_watcher_kind`**

Add an arm before the `"unknown"` fallback:

```rust
} else if rel.starts_with("scripts/") {
    "script"
}
```

- [ ] **Step 3: Add script routing in `process_change`**

Add a new arm in `process_change` after the diary check:

```rust
} else if rel_str.starts_with("scripts/") {
    process_script_change(db, workspace, path).await
}
```

- [ ] **Step 4: Write `process_script_change` function**

Add after `process_diary_change`, following the reference pattern (simpler — no topic
resolution, no frontmatter parsing):

```rust
/// Sync a changed script file to the database.
///
/// Scripts are simpler than notes: no frontmatter, no wiki links.
/// The file content IS the script. Path is relative to workspace
/// (e.g. `scripts/finance/spending.py`).
async fn process_script_change(
    db: &GhostDb,
    workspace: &Path,
    path: &Path,
) -> Result<Option<EmbedRequest>, PipelineError> {
    let fs_rel = path
        .strip_prefix(workspace)
        .unwrap_or(path)
        .to_string_lossy()
        .to_string();

    // DB stores path without the leading "scripts/" prefix
    let script_path = fs_rel
        .strip_prefix("scripts/")
        .unwrap_or(&fs_rel)
        .to_string();

    // Deletion
    if !path.exists() {
        if let Ok(Some(script)) =
            crate::db::knowledge::find_script_by_path(db, &script_path).await
        {
            crate::db::embeddings::delete_embeddings_for_source(db, &script.id).await?;
            crate::db::knowledge::delete_script(db, &script.id).await?;
            logfire::info!("watcher: deleted script", path = script_path);
        }
        return Ok(None);
    }

    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return Ok(None),
    };

    let script_id = match crate::db::knowledge::find_script_by_path(db, &script_path).await
    {
        Ok(Some(s)) => {
            let _ = crate::db::knowledge::update_script(db, &s.id, &content).await;
            s.id
        }
        _ => match crate::db::knowledge::create_script(db, &script_path, &content).await {
            Ok(id) => id,
            Err(_) => return Ok(None),
        },
    };

    Ok(Some(EmbedRequest {
        source_table: "script".into(),
        source_id: script_id,
        content,
        tags: vec![],
        topic_id: None,
        path: Some(fs_rel),
    }))
}
```

Note: `path` in `EmbedRequest` is set to the full filesystem-relative path (e.g.
`scripts/finance/spending.py`) so the code chunker can detect the language from the file
extension.

- [ ] **Step 5: Verify compilation**

Run: `cargo check`

- [ ] **Step 6: Commit**

```bash
git add src/daemon/watcher.rs
git commit -m "feat: filesystem watcher support for scripts/"
```

---

### Task 7: Embedding reconciliation — script support

**Files:**

- Modify: `src/embeddings/pipeline.rs`

Two changes: `reconcile_filesystem` and `reconcile_embeddings`.

- [ ] **Step 1: Add `"scripts"` to `reconcile_filesystem`**

In the `for subdir in ["notes", "references", "diary"]` line, add `"scripts"`:

```rust
for subdir in ["notes", "references", "diary", "scripts"] {
```

That's it — `process_change` (which we modified in Task 6) handles the routing.

- [ ] **Step 2: Add `path` parameter to `embed_source` and `embed_source_inner`**

Currently `embed_source_inner` (line 91) calls `chunk_content(content, tags, None)` —
the `None` means no AST-aware code chunking. Scripts need the path so tree-sitter can
detect Python/Bash. Thread a `path` parameter through:

In `embed_source` (line 32), add `path: Option<&str>` after `topic_id`:

```rust
pub async fn embed_source(
    client: &EmbeddingClient,
    db: &GhostDb,
    source_table: &str,
    source_id: &str,
    content: &str,
    tags: &[String],
    topic_id: Option<&str>,
    path: Option<&str>,        // NEW
) -> Result<usize, PipelineError> {
```

Pass it through to `embed_source_inner`.

In `embed_source_inner` (line 91), add `path: Option<&str>` and use it:

```rust
async fn embed_source_inner(
    // ... existing params ...
    path: Option<&str>,        // NEW
) -> Result<usize, PipelineError> {
    let chunks = chunk_content(content, tags, path);  // was None
    // ... rest unchanged ...
}
```

Do the same for `embed_source_forced`.

Update all existing callers (notes, references, diary reconciliation) to pass `None` for
`path` — their behavior is unchanged.

- [ ] **Step 3: Add script reconciliation to `reconcile_embeddings`**

After the diary reconciliation block (the `// Reconcile diary entries` section), add:

```rust
// Reconcile scripts (paginated)
let t = std::time::Instant::now();
let mut offset = 0;
loop {
    let scripts =
        db::knowledge::list_scripts_page(db, offset, RECONCILE_PAGE_SIZE).await?;
    let batch_len = scripts.len();
    for script in &scripts {
        let count = embed_source(
            client,
            db,
            "script",
            &script.id,
            &script.content,
            &[],
            None,
            Some(&format!("scripts/{}", script.path)),
        )
        .await?;
        if count > 0 {
            embedded += count;
        } else {
            skipped += 1;
        }
    }
    if batch_len < RECONCILE_PAGE_SIZE {
        break;
    }
    offset += batch_len;
}
tracing::info!(ms = t.elapsed().as_millis() as u64, "reconciled scripts");
```

- [ ] **Step 4: Verify compilation**

Run: `cargo check`

- [ ] **Step 5: Commit**

```bash
git add src/embeddings/pipeline.rs
git commit -m "feat: embedding reconciliation for scripts"
```

---

### Task 8: `knowledge_search` tool — scripts category

**Files:**

- Modify: `src/tools/knowledge_search.rs`

- [ ] **Step 1: Add `"scripts"` to the schema enum**

In the `schema()` method, update the `categories` items enum:

```json
"enum": ["notes", "references", "diary", "topics", "scripts"]
```

- [ ] **Step 2: Update the description**

Update the tool description to mention scripts:

```
"Search your knowledge base using hybrid BM25 + semantic search across notes,
references, diary entries, and scripts. Use this FIRST before web search to check
if you already have relevant information. Defaults to notes and diary; pass
categories to include references or scripts. Returns ranked results with snippets."
```

- [ ] **Step 3: Add the search flag computation**

After `search_topics_flag`, add:

```rust
let search_scripts_flag =
    !use_defaults && categories.iter().any(|c| c == "scripts");
```

- [ ] **Step 4: Add BM25 search for scripts**

After the `search_topics_flag` block that calls `search_topics`, add:

```rust
if search_scripts_flag {
    bm25_hits.extend(
        db::knowledge::search_scripts(&ctx.db, query, limit)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?,
    );
}
```

- [ ] **Step 5: Add to effective_categories**

After the `search_topics_flag` push, add:

```rust
if search_scripts_flag {
    effective_categories.push("scripts".to_string());
}
```

- [ ] **Step 6: Add to `filter_embedding_hits`**

In the `match c.as_str()` block, add:

```rust
"scripts" => "script",
```

- [ ] **Step 7: Add to `format_results`**

In the `kind_header` match, add:

```rust
"script" => "Scripts",
```

- [ ] **Step 8: Verify compilation**

Run: `cargo check`

- [ ] **Step 9: Run `just ci` to verify everything**

Run: `just ci`

Fix any clippy warnings or test failures.

- [ ] **Step 10: Commit**

```bash
git add src/tools/knowledge_search.rs
git commit -m "feat: knowledge_search supports scripts category"
```

---

## Chunk 3: Scripting Skill (Bundled Asset)

### Task 9: Create the scripting skill

**Files:**

- Create: `assets/skills/scripting/skill.md`

- [ ] **Step 1: Write the skill file**

Create `assets/skills/scripting/skill.md`:

```markdown
---
name: scripting
description:
  Write reusable scripts. Use when the OPERATOR asks a question that requires running
  code, data processing, API calls, or anything beyond a single coreutils command.
---

# Scripting

When answering a question that requires code, write a clean, reusable script rather than
a throwaway command. This makes your work reproducible and discoverable.

## Before Writing

Check if you already have a script for this:
```

knowledge_search query="<what you need>" categories=["scripts"]

````

If found, read it with `read_file` and run it. Update it if the OPERATOR's request is a
variation.

## File Organization

Scripts live in the workspace under `scripts/{topic}/{name}.py`:

- **topic**: broad category (e.g., `finance`, `domains`, `weather`, `system`)
- **name**: descriptive, lowercase with underscores (e.g., `spending_by_category.py`)

## Python Scripts (Default)

Use Python with [uv inline metadata (PEP 723)](https://docs.astral.sh/uv/guides/scripts/).
Every script MUST have:

1. **PEP 723 metadata block** — declares dependencies inline
2. **Module docstring** — what it does, when to use it (this powers search)
3. **typer** for scripts with arguments — gives automatic `--help`

### Template (with arguments)

```python
# /// script
# requires-python = ">=3.12"
# dependencies = ["httpx", "typer"]
# ///
"""Short description of what this script does.

Longer explanation of when to use it and what it expects.
"""

import typer

def main(
    arg1: str = typer.Argument(help="Description of arg1"),
    flag: bool = typer.Option(False, help="Description of flag"),
):
    """One-line description for --help."""
    ...

if __name__ == "__main__":
    typer.run(main)
````

### Template (no arguments)

```python
# /// script
# requires-python = ">=3.12"
# dependencies = ["httpx"]
# ///
"""Short description of what this script does.

Longer explanation.
"""

def main():
    ...

if __name__ == "__main__":
    main()
```

### Running

```
run_shell_command command="uv run scripts/{topic}/{name}.py [args]"
```

## Non-Python Scripts

Use Bash only for thin wrappers around existing CLI tools with no complex logic:

```bash
#!/usr/bin/env bash
# Short description of what this script does.
set -euo pipefail

# ... implementation
```

For other languages (Go, Rust), add required toolchains to the workspace nix shell
(`shell/`) and document the build step in the script header.

## When NOT to Script

- Single coreutils command (`ls`, `grep`, `wc`, `df`, `du`) → just run it
- Quick one-liner with no dependencies → just run it

## When to Script

- Needs a Python library (parsing, HTTP, data processing)
- Has logic (conditionals, loops over data, error handling)
- Would benefit from reuse (OPERATOR might ask again)
- Needs structured output (tables, formatted reports)

## Library Documentation

If you're unsure about a library's API, import its documentation first:

```
# For PyPI packages with docs sites:
run_shell_command command="ghost reference import crawl --url https://typer.tiangolo.com/ --topic typer --max-depth 2"

# For GitHub repos:
run_shell_command command="ghost reference import git --url https://github.com/tiangolo/typer --topic typer --paths docs/ --extensions md"
```

Then search your references:

```
knowledge_search query="typer argument" categories=["references"] topic="typer"
```

````

- [ ] **Step 2: Verify the skill is installed on workspace bootstrap**

Check how other skills in `assets/skills/` get installed. The `install_bundled_files` or
`bootstrap_workspace` function should pick it up automatically by scanning `assets/skills/`.
If not, you may need to add it explicitly — read `src/config_workspace.rs` to verify.

- [ ] **Step 3: Verify compilation**

Run: `cargo check`

(The skill is a static asset — compilation just ensures it's bundled correctly via
`include_dir` or similar.)

- [ ] **Step 4: Commit**

```bash
git add assets/skills/scripting/skill.md
git commit -m "feat: bundled scripting skill for GHOST"
````

---

## Chunk 4: E2e Test Infrastructure + Scripting Tests

### Task 10: Reorganize daemon e2e tests into folder

**Files:**

- Create: `tests/daemon.rs` (feature-gated entry point)
- Create: `tests/daemon/helpers.rs` (extracted helpers)
- Create: `tests/daemon/ark_nova.rs` (existing test, renamed)
- Delete content from: `tests/daemon_e2e.rs` (remove file or redirect)

- [ ] **Step 1: Create `tests/daemon.rs` entry point**

```rust
#![cfg(feature = "live-tests")]

mod common;
mod daemon;
```

- [ ] **Step 2: Create `tests/daemon/mod.rs`**

```rust
mod helpers;

mod ark_nova;
mod scripting;
```

- [ ] **Step 3: Create `tests/daemon/helpers.rs`**

Extract any shared daemon test setup. For now, re-export what's needed:

```rust
pub use crate::common::live_test_database;
```

- [ ] **Step 4: Move the existing test to `tests/daemon/ark_nova.rs`**

```rust
use super::helpers::live_test_database;

/// Test: import a PDF reference, verify it gets chunked, embedded, and is
/// searchable with relevant snippets.
#[tokio::test]
async fn test_ark_nova_import() {
    let env = live_test_database("ark_nova_import").await;
    let daemon = env.boot_daemon().await;

    let session_id = ghost::db::sessions::create_session(&daemon.db)
        .await
        .expect("create session");

    daemon
        .session_chat
        .chat(
            &session_id,
            "Import the Ark Nova rules for future reference",
            None,
            None,
        )
        .await
        .expect("chat failed");

    daemon.settle().await.expect("settle after chat");

    daemon.trigger_idle_agents().await;
    daemon.settle().await.expect("settle after reflection");

    let ref_count = ghost::db::knowledge::count_references(&daemon.db)
        .await
        .expect("count references");
    assert!(
        ref_count > 0,
        "expected at least one reference after import, got {ref_count}"
    );

    let embedding_count = ghost::db::embeddings::count_embeddings(&daemon.db)
        .await
        .expect("count embeddings");
    assert!(
        embedding_count >= 50,
        "expected 50+ embedding chunks, got {embedding_count}"
    );

    let results =
        ghost::db::knowledge::search_references(&daemon.db, "ark nova break rules", 10, None)
            .await
            .expect("reference search");

    let all_snippets: String = results
        .iter()
        .map(|r| r.snippet.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        all_snippets.to_lowercase().contains("break"),
        "search for 'ark nova break rules' should return snippets mentioning breaks.\n\
         Got {} results with snippets:\n{all_snippets}",
        results.len()
    );

    env.log_session_json("ark_nova_chat", &session_id).await;

    daemon.shutdown().await;
}
```

- [ ] **Step 5: Delete old `tests/daemon_e2e.rs`**

Remove the file entirely (its content has been moved).

- [ ] **Step 6: Verify the moved test still works**

Run:
`cargo test --features live-tests test_ark_nova_import -- --nocapture 2>&1 | tail -5`

Should compile and find the test (may skip actually running if no Ollama/provider).

- [ ] **Step 7: Commit**

```bash
git rm tests/daemon_e2e.rs
git add tests/daemon.rs tests/daemon/
git commit -m "refactor: reorganize daemon e2e tests into tests/daemon/ folder"
```

---

### Task 11: Create mock CSV fixture

**Files:**

- Create: `tests/fixtures/mock_bank_statement.csv`

- [ ] **Step 1: Write the fixture**

Create `tests/fixtures/mock_bank_statement.csv`:

```csv
date,description,amount,category
2026-03-01,Whole Foods Market,-45.23,groceries
2026-03-02,Tokyo Ramen House,-18.50,restaurants
2026-03-03,Monthly Rent,-1500.00,rent
2026-03-04,Electric Bill,-89.00,utilities
2026-03-05,Trader Joe's,-62.15,groceries
2026-03-06,Pizza Palace,-24.00,restaurants
2026-03-07,Water Bill,-45.00,utilities
2026-03-08,Safeway,-38.90,groceries
2026-03-09,Sushi Bar,-32.00,restaurants
2026-03-10,Internet Service,-70.00,utilities
2026-03-11,Farmers Market,-28.50,groceries
2026-03-12,Thai Express,-15.75,restaurants
```

Food total (groceries + restaurants): 45.23 + 18.50 + 62.15 + 24.00 + 38.90 + 32.00 +
28.50 + 15.75 = **265.03**

- [ ] **Step 2: Commit**

```bash
git add tests/fixtures/mock_bank_statement.csv
git commit -m "test: add mock bank statement CSV fixture for scripting tests"
```

---

### Task 12: Scripting e2e tests

**Files:**

- Create: `tests/daemon/scripting.rs`

- [ ] **Step 1: Write the scripting test module**

Create `tests/daemon/scripting.rs` with all three test scenarios:

```rust
use std::time::Duration;

use super::helpers::live_test_database;

/// Helper: assert a file exists in the workspace under scripts/ and return its content.
fn find_script(env: &crate::common::LiveTestEnv, subdir: &str) -> Option<String> {
    let scripts_dir = format!("scripts/{subdir}");
    // List files under scripts/{subdir}/
    let workspace = env.workspace_path();
    let dir = workspace.join("scripts").join(subdir);
    if !dir.exists() {
        return None;
    }
    for entry in std::fs::read_dir(&dir).ok()?.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("py") {
            return std::fs::read_to_string(&path).ok();
        }
    }
    None
}

/// Assert standard scripting conventions on a Python script.
fn assert_script_conventions(content: &str, test_name: &str) {
    assert!(
        content.contains("# /// script"),
        "[{test_name}] missing PEP 723 metadata block"
    );
    assert!(
        content.contains("# ///"),
        "[{test_name}] missing PEP 723 closing marker"
    );
    assert!(
        content.contains("\"\"\""),
        "[{test_name}] missing module docstring"
    );
}

// ---------------------------------------------------------------------------
// US1: Monthly spending from bank CSV
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_script_csv_spending() {
    let env = live_test_database("script_csv_spending").await;
    let daemon = env.boot_daemon().await;

    // Copy the CSV fixture into the workspace
    let fixture = std::path::Path::new("tests/fixtures/mock_bank_statement.csv");
    let dest = env.workspace_path().join("bank_statement.csv");
    std::fs::copy(fixture, &dest).expect("copy CSV fixture");

    let session_id = ghost::db::sessions::create_session(&daemon.db)
        .await
        .expect("create session");

    let timeout = Duration::from_secs(180);
    tokio::time::timeout(timeout, async {
        daemon
            .session_chat
            .chat(
                &session_id,
                "I have a bank statement at bank_statement.csv — how much did I spend \
                 on food this month? Break it down by category (groceries vs restaurants).",
                None,
                None,
            )
            .await
            .expect("chat failed");
    })
    .await
    .expect("TIMEOUT: script_csv_spending exceeded 180s");

    daemon.settle().await.expect("settle");

    // Assert: a script was created under scripts/
    // The GHOST might pick "finance", "spending", "budget" etc as topic
    let script_content = ["finance", "spending", "budget", "bank"]
        .iter()
        .find_map(|topic| find_script(&env, topic));

    let content = script_content.expect(
        "expected a Python script under scripts/{finance,spending,budget,bank}/",
    );
    assert_script_conventions(&content, "csv_spending");

    // Assert: script uses typer (has CLI arguments for category/file)
    assert!(
        content.contains("typer"),
        "expected script to use typer for CLI arguments"
    );

    env.log_session_json("csv_spending", &session_id).await;
    daemon.shutdown().await;
}

// ---------------------------------------------------------------------------
// US2: Domain expiry check
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_script_domain_expiry() {
    let env = live_test_database("script_domain_expiry").await;
    let daemon = env.boot_daemon().await;

    let session_id = ghost::db::sessions::create_session(&daemon.db)
        .await
        .expect("create session");

    let timeout = Duration::from_secs(180);
    tokio::time::timeout(timeout, async {
        daemon
            .session_chat
            .chat(
                &session_id,
                "Check if tolki.dev and tachikoma-ai.com are expiring soon",
                None,
                None,
            )
            .await
            .expect("chat failed");
    })
    .await
    .expect("TIMEOUT: script_domain_expiry exceeded 180s");

    daemon.settle().await.expect("settle");

    let script_content = ["domains", "dns", "whois"]
        .iter()
        .find_map(|topic| find_script(&env, topic));

    let content = script_content.expect(
        "expected a Python script under scripts/{domains,dns,whois}/",
    );
    assert_script_conventions(&content, "domain_expiry");

    assert!(
        content.contains("whois") || content.contains("python-whois"),
        "expected script to use whois library"
    );

    env.log_session_json("domain_expiry", &session_id).await;
    daemon.shutdown().await;
}

// ---------------------------------------------------------------------------
// US3: Weather forecast
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_script_weather_forecast() {
    let env = live_test_database("script_weather_forecast").await;
    let daemon = env.boot_daemon().await;

    let session_id = ghost::db::sessions::create_session(&daemon.db)
        .await
        .expect("create session");

    let timeout = Duration::from_secs(180);
    tokio::time::timeout(timeout, async {
        daemon
            .session_chat
            .chat(
                &session_id,
                "What's the weather going to be like this week near Tokyo station, Tokyo?",
                None,
                None,
            )
            .await
            .expect("chat failed");
    })
    .await
    .expect("TIMEOUT: script_weather_forecast exceeded 180s");

    daemon.settle().await.expect("settle");

    let script_content = ["weather", "forecast", "meteo"]
        .iter()
        .find_map(|topic| find_script(&env, topic));

    let content = script_content.expect(
        "expected a Python script under scripts/{weather,forecast,meteo}/",
    );
    assert_script_conventions(&content, "weather_forecast");

    assert!(
        content.contains("httpx") || content.contains("requests") || content.contains("urllib"),
        "expected script to use an HTTP client library"
    );

    env.log_session_json("weather_forecast", &session_id).await;
    daemon.shutdown().await;
}
```

- [ ] **Step 2: Check if `workspace_path()` exists on `LiveTestEnv`**

The test uses `env.workspace_path()`. Check `tests/common.rs` — if it doesn't exist, add
a simple getter:

```rust
pub fn workspace_path(&self) -> &Path {
    self.workspace.path()
}
```

Or use whatever existing method exposes the workspace directory (could be
`env.config.workspace` — check and adapt).

- [ ] **Step 3: Verify compilation**

Run: `cargo test --features live-tests --no-run 2>&1 | tail -10`

(Just compile, don't run — the tests need a live provider.)

- [ ] **Step 4: Commit**

```bash
git add tests/daemon/scripting.rs
git commit -m "test: e2e scripting tests (CSV spending, domain expiry, weather forecast)"
```

---

### Task 13: Final integration — `just ci`

- [ ] **Step 1: Run full CI**

Run: `just ci`

Fix any compilation errors, clippy warnings, or test failures.

- [ ] **Step 2: Commit any fixes**

```bash
git add -u
git commit -m "fix: address ci issues from scripts feature"
```

- [ ] **Step 3: Verify the skill is bundled**

Boot a test workspace and check that `skills/scripting/skill.md` exists:

```bash
cargo test --lib -- config_workspace 2>&1 | tail -5
```

Or manually verify:

```bash
cargo run -- init --workspace /tmp/ghost-test-scripts 2>/dev/null
ls /tmp/ghost-test-scripts/skills/scripting/
rm -rf /tmp/ghost-test-scripts
```

---

## Implementation Notes

- **Migration numbering**: Next migration is `008_scripts.sql` (after
  `007_fix_reference_fts.sql`).
- **`embed_source` path threading**: Task 7 adds `path: Option<&str>` to `embed_source`
  and `embed_source_inner`. All existing callers (notes, references, diary) pass `None`.
  Only scripts pass the file path for AST-aware code chunking.
- **Test helper `workspace_path`**: Task 12 needs access to the workspace directory.
  Check what `LiveTestEnv` already exposes before adding a new method.
- **Skill installation**: The `assets/skills/` directory should be auto-installed by
  `install_bundled_files`. Verify by checking how `assets/skills/coding/skill.md` gets
  installed.
- **`list_recent`**: The plan does not add scripts to `list_recent()` in `crud.rs`. This
  is intentional — scripts are utility code, not knowledge the GHOST browses
  chronologically. Can be added later if needed.
