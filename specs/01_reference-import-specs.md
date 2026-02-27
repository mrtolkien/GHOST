# Reference Import Tool — Implementation Plan

Companion to `specs/01_reference-import.md` (the feature spec). This file is the full
implementation plan, including lessons from the failed first attempt (Feb 2026).

## Prior art: t-koma

The predecessor project (`../t-koma`) had a working reference import pipeline. Key
files:

- `t-koma-knowledge/src/sources.rs` — clone, sparse checkout, file walk
- `t-koma-knowledge/tests/reference_topic_e2e.rs` — passing end-to-end test that clones
  DioxusLabs/docsite, indexes, and searches

---

## Step 1: Schema changes

Add these fields to the database schema (adapt SQL syntax to whatever DB backend is
current at implementation time):

### Reference table — versioning

```
version_ref TEXT      -- git commit hash for git sources, NULL for web
fetched_at  DATETIME  -- import timestamp for staleness detection
```

### Embedding table — topic scope

```
topic TEXT  -- set from the reference's topic; NULL for notes/diary
```

Enables topic-scoped vector search with a WHERE clause (no post-filter needed since we
brute-force scan, not HNSW).

### Full-text index on topic

Add a BM25 index on the `topic` field of the reference table so
`knowledge_search(query="dioxus", categories=["references"])` finds topic index pages.

### Record types to update

- `ReferenceRecord`: add `version_ref: Option<String>`,
  `fetched_at: Option<DateTime<Utc>>`
- `EmbeddingHit`: add `topic: Option<String>`

---

## Step 2: Thread topic through embedding pipeline

### DB layer (`db/embeddings`)

- `upsert_embedding()` — add `topic: Option<&str>` param, include in INSERT + upsert
- `vector_search()` — add `topic: Option<&str>` param; when set, add
  `WHERE topic = $topic` before cosine similarity

### Embedding pipeline (`embeddings/pipeline`)

- `embed_source()` / `embed_source_forced()` / `embed_source_inner()` — add
  `topic: Option<&str>`, thread to `upsert_embedding()`
- `reconcile_embeddings()` — pass `Some(&ref.topic)` for references, `None` for
  notes/diary

### Update all callers

- CLI `cmd_reindex()` — thread topic from reference record
- Any tool that calls `embed_source` — pass `None` for non-references

---

## Step 3: Code-aware chunking (cAST-inspired)

### New: `src/embeddings/code_chunker.rs`

Custom tree-sitter chunker inspired by the cAST algorithm (CMU, 2025). ~200 lines.
Algorithm:

1. Parse source with tree-sitter, get AST root
2. Recursive descent: for each node, if it fits in CHUNK_TARGET → emit as a chunk. If
   it's oversized → recurse into its children
3. Greedy sibling merge: pack consecutive small siblings into one chunk up to
   CHUNK_TARGET
4. **Metadata prepend**: each chunk gets a header like
   `[file: src/chat/tool_loop.rs] [language: rust] [scope: impl ToolLoopHandler > handle_tool_call]`
   — this dramatically improves embedding quality
5. Fallback: unknown language → existing `chunk_text()`

### `src/embeddings/chunker.rs`

- Add `chunk_code(content, file_path, tags) -> Vec<Chunk>` that delegates to
  `code_chunker.rs` when language is detected, else falls back to `chunk_text()`
- Language detection from file extension (`.rs` → Rust, `.py` → Python)

### Dependencies

```toml
tree-sitter = "0.24"
tree-sitter-rust = "0.23"
tree-sitter-python = "0.23"
tree-sitter-javascript = "0.23"
tree-sitter-typescript = "0.23"
tree-sitter-go = "0.23"
tree-sitter-java = "0.23"
tree-sitter-c = "0.23"
tree-sitter-bash = "0.23"
tree-sitter-toml = "0.7"
tree-sitter-json = "0.24"
```

**Note:** the first attempt confirmed these all compile and the chunker works correctly.
Keep this implementation as-is.

---

## Step 4: Topic-scoped knowledge search

### `db/knowledge/search`

- `search_references()` — add `topic: Option<&str>`, append `AND topic = $topic` to
  WHERE clause when provided

### `tools/knowledge_search`

- **Schema**: add `"topic"` string parameter
- **`execute()`**: extract topic, pass to `search_references()` and `vector_search()`
  (via the new topic param)
- **`filter_embedding_hits()`**: when topic provided, filter by `EmbeddingHit.topic`

### CLI

- `cmd_search()`: add `--topic` CLI flag

---

## Step 5: DB helpers

### `db/knowledge/crud`

- `create_reference()` — add `version_ref: Option<&str>` and
  `fetched_at: Option<DateTime<Utc>>` params
- `delete_references_by_topic(db, topic)` — bulk delete all references for a topic +
  cascade delete their embeddings
- `count_references_by_topic(db, topic)` — for progress display
- `list_topics(db)` — returns distinct topic names with counts

---

## Step 6: Reference import core — `src/reference_import/`

### Types (`types.rs`)

```rust
pub struct ImportConfig {
    pub source: ImportSource,
    pub topic: String,
}

pub enum ImportSource {
    Git { url: String, paths: Vec<String>, extensions: Vec<String> },
    Page { url: String },
    Crawl { url: String, max_depth: usize, max_pages: usize },
}

pub struct ImportResult {
    pub references_created: usize,
    pub references_skipped: usize,
    pub embeddings_generated: usize,
}
```

### Git import (`git.rs`) — USES TWO-PHASE CLONE

**Critical lesson from first attempt:** never use a plain `git clone --depth 1`. For
large repos (DioxusLabs/docsite is 315MB), this downloads the entire repo even when you
only need a few files.

Use the t-koma two-phase pattern instead:

```rust
pub async fn import_git(...) -> Result<ImportResult, ImportError> {
    let tmpdir = tempfile::TempDir::new()?;
    let repo_dir = tmpdir.path().join("repo");

    // Phase 1: blobless clone WITHOUT checkout.
    // --filter=blob:none fetches only tree objects (tiny).
    // --no-checkout prevents the default checkout from fetching
    // all blobs.
    Command::new("git")
        .args([
            "clone", "--no-checkout", "--depth", "1",
            "--filter=blob:none", url,
        ])
        .arg(&repo_dir)
        .output().await?;

    // Phase 2: sparse checkout — only materialise the dirs we need.
    // This triggers blob fetches ONLY for files in these paths.
    if !paths.is_empty() {
        let dirs: Vec<&str> = paths.iter()
            .filter(|p| p.ends_with('/'))
            .map(|s| s.as_str())
            .collect();
        if !dirs.is_empty() {
            run_git(&repo_dir, &["sparse-checkout", "init", "--cone"]).await?;
            let mut args = vec!["sparse-checkout", "set"];
            args.extend_from_slice(&dirs);
            run_git(&repo_dir, &args).await?;
        }
        // Now checkout — fetches blobs for sparse paths only
        run_git(&repo_dir, &["checkout"]).await?;
    }

    // Get commit hash for version_ref
    let hash = run_git_output(&repo_dir, &["rev-parse", "HEAD"]).await?;

    // Walk tree, filter by path prefixes + extensions
    // Per matching file:
    //   - chunk_code() for code, chunk_text() for .md
    //   - create_reference() with version_ref + fetched_at
    //   - collect EmbedRequest with topic
    //   - skip if reference path already exists (idempotent)
    // Batch embed all references at the end
    // Create/update topic note (see below)
}

/// Always use .current_dir() — never the -C flag.
async fn run_git(repo_dir: &Path, args: &[&str]) -> Result<(), ImportError> {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo_dir)
        .output().await?;
    // check output.status.success()
}
```

Dep: `tempfile` (ensure in main deps, not just dev-deps).

### Page import (`page.rs`)

1. Fetch URL with `web::fetch::fetch()` (reuse existing pipeline)
2. Store as reference: `path` = URL slug, `fetched_at` = now
3. Embed with topic
4. Create/update topic note

### Crawl import (`crawl.rs`)

BFS crawler:

1. `VecDeque<(Url, usize)>` queue + `HashSet<String>` visited
2. Fetch each URL with same logic as web_fetch tool (crawl4ai fallback, readability on
   large pages)
3. Extract same-host links: regex on markdown for `[...](url)`, `Url::join()` for
   relative, filter to same host
4. Respect `max_depth` and `max_pages`
5. Store + embed each page as a reference
6. Create/update topic note

### Topic note (`topic_note.rs`)

After import, auto-create a **topic note** (archetype: topic) so the topic is
discoverable via default `knowledge_search` (which searches notes + diary by default,
NOT references).

Why a note instead of an `_index` reference: `knowledge_search` defaults to
`["notes", "diary"]`. A topic note is found automatically when the GHOST searches "rust
frontend library". An \_index reference would only be found if the GHOST explicitly
passes `categories=["references"]`, which it wouldn't do unless it already knew the
topic existed — chicken-and-egg.

Auto-generated skeleton (no LLM needed):

```markdown
---
title: Dioxus
archetype: topic
tags:
  - dioxus
sources:
  - https://github.com/DioxusLabs/docsite
trust: 3
---

Reference hub for Dioxus.

## Collections

- `dioxus/docs`: 47 pages imported from https://github.com/DioxusLabs/docsite on
  2026-02-25 (commit abc123)
```

- `trust: 3` — shell note, not yet enriched
- The **skill** instructs the GHOST to enrich this note after import with a
  natural-language description of what the library does (makes it findable through
  semantic search)
- If a topic note already exists, update the Collections section
- Derive library name from topic: `dioxus/docs` → title "Dioxus", tag `dioxus`

---

## Step 7: CLI — `src/cli/reference.rs`

```
ghost reference import --source git --url <url> --topic <topic> \
  [--paths doc/,README.md] [--extensions .md,.rs]
ghost reference import --source page --url <url> --topic <topic>
ghost reference import --source crawl --url <url> --topic <topic> \
  [--max-depth 3] [--max-pages 50]
ghost reference topics
ghost reference search "rust frontend library"
ghost reference delete --topic <topic>
```

### `topics` subcommand

Lists all topics grouped by library, with counts and import metadata:

```
dioxus
  dioxus/docs   47 refs  imported 2026-02-25  (git: abc123)
  dioxus/source 120 refs  imported 2026-02-24  (git: def456)
surrealdb
  surrealdb/docs  32 refs  imported 2026-02-20  (crawl)
```

Implementation: `list_topics(db)` returns distinct topics with counts. Group by first
path segment (library name).

### `search` subcommand

Semantic search over topic notes (archetype: topic), returning matched topics with their
reference collections.

### Wiring

- `src/cli/reference.rs` — new file, clap subcommands
- `src/cli/mod.rs` — add `pub mod reference;`
- `src/main.rs` — add `Reference` variant + dispatch
- `src/lib.rs` — add `pub mod reference_import;`

---

## Step 8: Skills

### Enhance `prompts/skills/knowledge-navigator.md`

Update description to trigger on library/SDK/tool questions. Add a "Reference Topics"
section teaching topic-scoped search:

1. Search for the topic note (default `knowledge_search` finds it)
2. Search within a topic with the `topic=` parameter
3. Browse topics via `ghost reference topics`
4. Always check imported references before falling back to web search

### New: `prompts/skills/reference-import.md`

Skill that teaches the GHOST how to suggest and run reference imports:

- CLI syntax for git/page/crawl
- When to suggest each source type
- Post-import flow: enrich the topic note with a description
- Add to `DEFAULT_SKILLS` in `src/skills.rs`

### System prompt — research escalation

Update `prompts/chat-system.md` escalation levels to mention topic notes as a first stop
for library questions.

---

## Step 9: Tests

### 9a. Git import — `live-tests` feature

**`tests/reference_import_git.rs`**

- Clone `DioxusLabs/docsite` with path filter `docs-src/0.7/src/tutorial/` and extension
  `.md`
- This yields ~15 small markdown files
- Assert: `references_created > 0`, `version_ref` set (commit hash), `fetched_at` set,
  paths prefixed with topic
- Assert: topic note created (archetype: topic, correct title/tags, Collections section
  mentions the topic)
- Assert: re-import is idempotent (`references_created == 0`,
  `references_skipped == first_run.references_created`)

**Note:** the two-phase clone (Step 6) is critical here. The docsite repo is 315MB.
Without `--no-checkout --filter=blob:none` + sparse checkout, the test will either
timeout or OOM.

### 9b. Crawl import — `live-tests` feature

**`tests/reference_import_crawl.rs`**

- Crawl `https://ghost.tolki.dev/` with `max_depth=2`, `max_pages=5`
- Assert: `references_created > 0` and `<= max_pages`
- Assert: `source_url` set on each reference
- Assert: topic note created
- Assert: re-crawl skips existing (idempotent)

### 9c. Skill discovery — `e2e-tests` feature

**`tests/reference_import_discovery.rs`**

- Chat session: user asks "How do I create components in Dioxus?"
- Assert: GHOST reads a skill file
- Assert: GHOST calls `knowledge_search` before answering
- Soft assertion for reference-import suggestion (model might answer from training data)

### 9d. Shared step-based e2e harness (required baseline)

All future `e2e-tests` scenarios (including reference-import scenarios) should use the
step-based harness in `tests/e2e/`:

- One test per action boundary (`step_01_*`, `step_02_*`, ...)
- Hard fail if previous step fixture is missing
- Full workspace snapshot persisted between steps as `workspace.tar.zst`
- Step artifacts per model:
  - `state.json` (session ids, assertion markers, previews)
  - `transcript.json` + `transcript.md` (readable log with tool calls/results +
    thinking)
  - `metrics.json`
- Manual refresh only: `uv run scripts/e2e refresh --models <aliases>`
- Sequential execution only (`--test-threads=1`)

Fixture root:

```text
tests/fixtures/e2e/<scenario>/<model_alias>/step_XX_<name>/
```

### 9e. Reference-import scenario onboarding (deferred)

After Step 1-8 implementation is complete, add a dedicated reference-import scenario to
the shared harness:

- Step 01: import Dioxus docs reference topic
- Step 02: ask a Dioxus question in chat
- Step 03: verify topic-scoped retrieval and response quality
- Optional Step 04: reflection checks on produced notes/references

This is intentionally deferred until the reference import pipeline is implemented.

### 9f. Post-import search — NO feature flag

**`tests/reference_import_search.rs`**

This test uses `test_database()` (no network, no real provider) and pre-populates the DB
with fake references + fake 1024-dim embeddings.

- Pre-populate references under two topics (`dioxus/docs`, `surrealdb/api`)
- Create a topic note
- Assert: `search_references` with topic filter returns scoped results
- Assert: `search_notes` finds topic note via BM25
- Assert: `list_topics` returns correct counts
- Assert: `vector_search` with topic filter returns scoped results
- Assert: `delete_references_by_topic` cascades to embeddings
- Assert: `count_references_by_topic` returns 0 after delete

**This test passed in the first attempt.** It's independent of the DB backend migration.

---

## File Summary

| Action | File                                                    |
| ------ | ------------------------------------------------------- |
| New    | `src/reference_import/mod.rs`                           |
| New    | `src/reference_import/types.rs`                         |
| New    | `src/reference_import/git.rs`                           |
| New    | `src/reference_import/page.rs`                          |
| New    | `src/reference_import/crawl.rs`                         |
| New    | `src/reference_import/topic_note.rs`                    |
| New    | `src/embeddings/code_chunker.rs`                        |
| New    | `src/cli/reference.rs`                                  |
| New    | `prompts/skills/reference-import.md`                    |
| New    | `tests/reference_import_git.rs`                         |
| New    | `tests/reference_import_crawl.rs`                       |
| New    | `tests/reference_import_search.rs`                      |
| New    | `tests/reference_import_discovery.rs`                   |
| New    | `tests/e2e_steps.rs`                                    |
| New    | `tests/e2e/harness.rs`                                  |
| New    | `scripts/e2e`                                           |
| New    | `scripts/e2e/launcher.py`                               |
| New    | `scripts/e2e/refresh.py`                                |
| New    | `scripts/e2e/render_log.py`                             |
| New    | `scripts/e2e/diff.py`                                   |
| New    | `scripts/e2e/analyze_request.py`                        |
| Modify | DB schema — versioning + topic fields                   |
| Modify | DB embeddings — topic in upsert/search/hit              |
| Modify | DB knowledge/search — topic filter                      |
| Modify | DB knowledge/crud — versioning, bulk ops, list_topics   |
| Modify | DB knowledge/records — new fields                       |
| Modify | Embedding pipeline — thread topic                       |
| Modify | Embedding chunker — add chunk_code()                    |
| Modify | `knowledge_search` tool — topic param                   |
| Modify | CLI knowledge — --topic on search                       |
| Modify | `src/main.rs` — Reference command                       |
| Modify | `src/cli/mod.rs` — add module                           |
| Modify | `src/lib.rs` — pub mod reference_import                 |
| Modify | `src/skills.rs` — add default skill                     |
| Modify | `prompts/skills/knowledge-navigator.md` — topic section |
| Modify | `prompts/chat-system.md` — escalation levels            |
| Modify | `Cargo.toml` — tree-sitter + tempfile                   |

---

## Verification checklist

1. `just ci` passes
2. `ghost reference import --source git --url <repo> --topic test/docs --extensions .md`
   → imports with commit hash, progress, topic note
3. `ghost reference topics` → shows topics with counts + dates
4. `ghost reference search "query"` → semantic search finds topic note
5. `ghost knowledge search --kind reference --topic test/docs "query"` → scoped results
6. `ghost reference delete --topic test/docs` → cleans up refs + embeddings + note
7. `cargo test` → reference_import_search passes (no feature flag)
8. `cargo test --features live-tests` → git + crawl import tests pass
9. Code chunks: import a .rs file, verify chunks split at function boundaries with
   metadata headers
