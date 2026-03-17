# Update References Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if
> subagents available) or superpowers:executing-plans to implement this plan. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `ghost reference update <topic>` CLI command that re-fetches references
from their original source, updates changed content, adds new files, and safely handles
upstream deletions (orphan protection for cited references).

**Architecture:** The update command reads stored import config from `_import.toml` (or
DB `import_batch`), re-fetches from the original source, diffs against existing
references by content hash, and applies changes. References deleted upstream but cited by
notes are moved to `_orphaned/` with a warning rather than deleted. A new `--ref` flag
on both `import` and `update` enables targeting specific git tags/branches.

**Tech Stack:** Rust, clap (CLI), sqlx (SQLite), tokio (async), git CLI (subprocess)

**Spec:** `backlog/tasks/3-invisible-improvements/2-update-references.md`

---

## File Structure

| Action | Path | Responsibility |
|--------|------|----------------|
| Create | `migrations/012_import_batch_config.sql` | Add `import_config` JSON column to `import_batch` |
| Modify | `src/reference_import/types.rs` | Add `git_ref` to `ImportSource::Git`, add `ImportConfigJson` serde type for persistence |
| Modify | `src/reference_import/topic.rs` | Extend `write_import_toml` to include all config params |
| Modify | `src/reference_import/git.rs` | Support `--ref` (branch/tag checkout), return file manifest for diffing |
| Modify | `src/reference_import/crawl.rs` | Return page manifest for diffing |
| Create | `src/reference_import/update.rs` | Core update logic: diff, apply, orphan handling |
| Modify | `src/reference_import/mod.rs` | Export new types and `update_references` |
| Modify | `src/db/knowledge/import_batch.rs` | Store/load `import_config` JSON |
| Modify | `src/db/knowledge/graph.rs` | Add `cited_reference_ids` batch query |
| Modify | `src/db/knowledge/mod.rs` | Re-export new functions |
| Modify | `src/cli/reference.rs` | Add `Update` subcommand, `--ref` flag on `Git` import |
| Modify | `assets/skills/reference-import/skill.md` | Document `update` command |
| Create | `tests/reference_update_git.rs` | Live test: import → mutate → update → verify diff |

---

## Task 1: Migration — add `import_config` JSON column

The `import_batch` table currently stores only `source_type`, `source_url`,
`version_ref`, `ref_count`. We need the full import config (paths, extensions,
max_depth, max_pages, git_ref) persisted so `update` can replay the import.

A JSON TEXT column is the simplest approach — matches the existing pattern (tags, sources
columns elsewhere use JSON TEXT).

**Files:**
- Create: `migrations/012_import_batch_config.sql`
- Modify: `src/db/knowledge/records.rs` — add `import_config` field to `ImportBatchRecord`
- Modify: `src/db/knowledge/import_batch.rs` — pass and store `import_config` in upsert

- [ ] **Step 1: Write the migration**

```sql
-- migrations/012_import_batch_config.sql
ALTER TABLE import_batch ADD COLUMN import_config TEXT;
```

- [ ] **Step 2: Add `import_config` field to `ImportBatchRecord`**

In `src/db/knowledge/records.rs`, add `pub import_config: Option<String>` to
`ImportBatchRecord`. This is a JSON string — parsed by the caller, not the record.

- [ ] **Step 3: Update `upsert_import_batch` to accept and store `import_config`**

In `src/db/knowledge/import_batch.rs`, add `import_config: Option<&str>` parameter to
`upsert_import_batch`. Add it to the INSERT column list and the ON CONFLICT UPDATE SET.
Update all existing call sites to pass `None` for now (they'll be wired in Task 3).

- [ ] **Step 4: Run `just ci` — fix any compilation errors**

All existing call sites must pass the new param. Grep for `upsert_import_batch` to find
them all: `git.rs`, `crawl.rs`, `page.rs`, `file.rs`.

- [ ] **Step 5: Commit**

```
feat: add import_config JSON column to import_batch
```

---

## Task 2: `--ref` flag for git import

Currently `import_git` always clones HEAD with `--depth 1`. We need `--ref <branch-or-tag>`
support to pin imports to a specific version.

**Files:**
- Modify: `src/reference_import/types.rs` — add `git_ref` field to `ImportSource::Git`
- Modify: `src/reference_import/git.rs` — use `--branch <ref>` in clone command
- Modify: `src/cli/reference.rs` — add `--ref` arg to `ReferenceImportCommand::Git`

- [ ] **Step 1: Add `git_ref` to `ImportSource::Git`**

In `src/reference_import/types.rs`:
```rust
Git {
    url: String,
    paths: Vec<String>,
    extensions: Vec<String>,
    git_ref: Option<String>,  // branch name or tag
}
```

Fix all pattern matches on `ImportSource::Git` (compiler will guide you).

- [ ] **Step 2: Use `git_ref` in clone command**

In `src/reference_import/git.rs`, if `git_ref` is `Some(r)`, add `"--branch", r` to the
clone args (before `--depth 1`). Git's `--branch` works for both branches and tags.

Store the resolved commit hash as `version_ref` (already done via `rev-parse HEAD`).

- [ ] **Step 3: Add `--ref` CLI arg**

In `src/cli/reference.rs`, add to `ReferenceImportCommand::Git`:
```rust
#[arg(long = "ref")]
git_ref: Option<String>,
```

Wire it into `ImportConfig` construction.

- [ ] **Step 4: Run `just ci`**

- [ ] **Step 5: Commit**

```
feat: add --ref flag to git reference import for branch/tag targeting
```

---

## Task 3: Persist full import config

Wire the full `ImportConfig` into `_import.toml` and `import_batch.import_config` so
that `update` can replay the exact same import.

**Files:**
- Modify: `src/reference_import/types.rs` — add `ImportConfigJson` serde struct
- Modify: `src/reference_import/topic.rs` — extend `write_import_toml` with full config
- Modify: `src/reference_import/git.rs` — pass config JSON to batch upsert + toml writer
- Modify: `src/reference_import/crawl.rs` — same

- [ ] **Step 1: Define `ImportConfigJson` serde type**

In `src/reference_import/types.rs`, add a serializable struct that captures the full
config for replay. This is what gets stored as JSON in `import_batch.import_config` and
also written into `_import.toml` under an `[import]` section:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportConfigJson {
    pub source_type: String,
    pub source_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_ref: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub paths: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub extensions: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_depth: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_pages: Option<usize>,
}
```

Add a `From<&ImportConfig>` impl to build it from the existing `ImportConfig`.

- [ ] **Step 2: Rewrite `write_import_toml` to use `toml` crate**

Replace the manual string formatting in `src/reference_import/topic.rs` with proper
`toml::to_string_pretty(&import_config_json)`. Keep the `# Auto-generated` header
comment. The function signature changes to accept `&ImportConfigJson` plus `version_ref`
and `ref_count` (these are runtime values, not config).

Define a small serializable wrapper struct for the TOML file:

```rust
#[derive(Serialize)]
struct ImportToml {
    source_type: String,
    source_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    version_ref: Option<String>,
    ref_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    git_ref: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    paths: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    extensions: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_depth: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_pages: Option<usize>,
}
```

- [ ] **Step 3: Update `import_git` and `import_crawl` to persist config**

In both `git.rs` and `crawl.rs`:
1. Build `ImportConfigJson` from the `ImportConfig`
2. Serialize to JSON string: `serde_json::to_string(&config_json)?`
3. Pass to `upsert_import_batch` as the new `import_config` param
4. Pass `&config_json` to the updated `write_import_toml`

Do the same for `page.rs` and `file.rs` if they call `write_import_toml` / `upsert_import_batch`.

- [ ] **Step 4: Run `just ci`**

- [ ] **Step 5: Commit**

```
feat: persist full import config in _import.toml and import_batch
```

---

## Task 4: Load saved config for replay

Add a function to read back the stored import config so `update` can reconstruct the
original `ImportConfig`.

**Files:**
- Modify: `src/reference_import/types.rs` — add `ImportConfigJson::to_import_config()` method
- Modify: `src/reference_import/topic.rs` — add `read_import_toml()` function

- [ ] **Step 1: Add `to_import_config` on `ImportConfigJson`**

```rust
impl ImportConfigJson {
    pub fn to_import_config(&self, topic: &str) -> Result<ImportConfig, ImportError> {
        let source = match self.source_type.as_str() {
            "git" => ImportSource::Git {
                url: self.source_url.clone(),
                paths: self.paths.clone(),
                extensions: self.extensions.clone(),
                git_ref: self.git_ref.clone(),
            },
            "crawl" => ImportSource::Crawl {
                url: self.source_url.clone(),
                max_depth: self.max_depth.unwrap_or(3),
                max_pages: self.max_pages.unwrap_or(50),
            },
            other => return Err(ImportError::Config(
                format!("unsupported source_type for update: {other}")
            )),
        };
        Ok(ImportConfig { source, topic: topic.to_string() })
    }
}
```

Note: `page` and `file` imports are one-shot (no upstream to re-fetch), so they return
an error from `to_import_config`.

- [ ] **Step 2: Add `Config` variant to `ImportError`**

In `src/reference_import/types.rs`, add to the `ImportError` enum:
```rust
#[error("config error: {0}")]
Config(String),
```

This is used by `to_import_config` for unsupported source types, by `read_import_toml`
for missing/malformed files, and by `update_references` for "topic not found".

- [ ] **Step 3: Add `read_import_toml()` with DB fallback**

In `src/reference_import/topic.rs`. Try `_import.toml` first; if missing, fall back to
the `import_batch.import_config` JSON column in the DB:

```rust
pub fn read_import_toml(
    workspace: &Path,
    topic_name: &str,
) -> Result<ImportConfigJson, ImportError> {
    let path = workspace.join("references").join(topic_name).join("_import.toml");
    let content = std::fs::read_to_string(&path)
        .map_err(|e| ImportError::Config(format!(
            "no _import.toml for topic '{topic_name}': {e}"
        )))?;
    toml::from_str(&content)
        .map_err(|e| ImportError::Config(format!(
            "invalid _import.toml for topic '{topic_name}': {e}"
        )))
}

/// Fallback: load config from DB import_batch.import_config JSON column.
pub async fn load_import_config_from_db(
    db: &GhostDb,
    topic_id: &str,
) -> Result<Option<ImportConfigJson>, ImportError> {
    let batch = db::knowledge::get_import_batch_by_topic(db, topic_id).await?;
    match batch.and_then(|b| b.import_config) {
        Some(json) => {
            let config: ImportConfigJson = serde_json::from_str(&json)
                .map_err(|e| ImportError::Config(format!("invalid import_config JSON: {e}")))?;
            Ok(Some(config))
        }
        None => Ok(None),
    }
}
```

- [ ] **Step 4: Export from `mod.rs`**

Add `read_import_toml`, `load_import_config_from_db`, and `ImportConfigJson` to the
public exports in `src/reference_import/mod.rs`.

- [ ] **Step 5: Run `just ci`**

- [ ] **Step 6: Commit**

```
feat: read back stored import config for replay
```

---

## Task 5: Citation-safe orphan detection

Before we can delete references that disappeared upstream, we need to check if any notes
cite them. Add a batch query and the orphan-move logic.

**Files:**
- Modify: `src/db/knowledge/graph.rs` — add `cited_reference_ids` batch query
- Modify: `src/db/knowledge/mod.rs` — re-export

- [ ] **Step 1: Add batch citation check**

In `src/db/knowledge/graph.rs`, add a function that takes a list of reference IDs and
returns the subset that have at least one citation:

```rust
pub async fn cited_reference_ids(
    db: &SqlitePool,
    reference_ids: &[String],
) -> Result<HashSet<String>, DatabaseError> {
    // For small batches, use IN clause; for large, use temp table
    // Pragmatic: build IN clause with bind params
}
```

Returns a `HashSet<String>` of reference IDs that are cited by at least one note.

- [ ] **Step 2: Run `just ci`**

- [ ] **Step 3: Commit**

```
feat: add batch citation check for orphan detection
```

---

## Task 6: Core update logic

This is the main `update_references` function. It re-fetches from source, diffs, and
applies changes.

**Files:**
- Create: `src/reference_import/update.rs`
- Modify: `src/reference_import/mod.rs` — export `update_references`
- Modify: `src/reference_import/types.rs` — add `UpdateResult`

- [ ] **Step 1: Define `UpdateResult`**

In `src/reference_import/types.rs`:
```rust
#[derive(Debug)]
pub struct UpdateResult {
    pub created: usize,
    pub updated: usize,
    pub deleted: usize,
    pub orphaned: usize,   // moved to _orphaned/ because cited
    pub unchanged: usize,
    pub old_version_ref: Option<String>,
    pub new_version_ref: Option<String>,
}
```

- [ ] **Step 2: Write `update_references` — the orchestrator**

Create `src/reference_import/update.rs`. The function:

```rust
pub async fn update_references(
    db: &GhostDb,
    workspace: &Path,
    topic_name: &str,
    ref_override: Option<&str>,  // CLI --ref override
) -> Result<UpdateResult, ImportError>
```

**Algorithm:**

1. Look up topic by name → error if not found
2. Read `_import.toml` via `read_import_toml(workspace, topic_name)`. If that fails,
   fall back to `load_import_config_from_db(db, topic_id)`. Error if both fail.
3. If `ref_override` is `Some`, override `git_ref` in the loaded config
4. Build `ImportConfig` via `config_json.to_import_config(topic_name)`
5. **Re-fetch from source** — call `fetch_git_manifest` or `fetch_crawl_manifest` (see
   steps 3-4 below)
6. **Git short-circuit:** If source is git and the new commit hash equals the stored
   `version_ref` (from the import batch) AND no `--ref` override was given, print
   "Already up to date at {hash}" and return early with all-zeros `UpdateResult`.
7. Load existing references from DB: `list_references_by_topic(db, Some(&topic_id), 10_000)`
   → build `HashMap<path, (ref_id, file_hash)>`
8. **Diff:**
   - For each upstream file: compute hash, compare against existing map
     - Not in map → **create** (write disk + DB)
     - In map, hash differs → **update** (overwrite disk, `update_reference` in DB)
     - In map, hash matches → **unchanged** (skip)
   - For each existing ref NOT in upstream manifest → **deleted upstream**
     - Check citations via `cited_reference_ids`
     - If cited → **orphan**: move disk file to `references/{topic}/_orphaned/{filename}`,
       update DB path via `update_reference_path`, print warning with citing note IDs
     - If not cited → **delete**: remove disk file, `delete_reference` from DB
   - **Note on disk writes:** For both create and update cases, explicitly `std::fs::write`
     the content to disk before updating the DB. The file watcher will pick up the change
     and trigger re-embedding.
9. Update batch metadata (new version_ref, ref_count)
10. Rewrite `_import.toml`
11. Return `UpdateResult`

**Crawl content instability:** Re-crawling a page may produce slightly different markdown
even when the page hasn't meaningfully changed (whitespace, timestamps, dynamic content).
This will cause false "updated" detections. This is acceptable for now — the cost is just
unnecessary re-embedding, not data corruption.

- [ ] **Step 3: Extract `fetch_git_manifest` from `import_git`**

The current `import_git` clones + walks + writes to DB in one pass. We need a lower-level
function that clones + walks and returns a manifest of `(rel_path, content)` pairs without
touching the DB. Refactor:

In `src/reference_import/git.rs`, extract:
```rust
pub(crate) async fn fetch_git_manifest(
    config: &ImportConfig,
) -> Result<(String, Vec<(String, String)>), ImportError>
//  Returns: (commit_hash, vec of (topic-relative path, content))
```

This does: clone → sparse checkout → `rev-parse HEAD` → walk files → read content →
return. Then refactor `import_git` to call `fetch_git_manifest` + the existing DB write
loop.

- [ ] **Step 4: Extract `fetch_crawl_manifest` from `import_crawl`**

Same pattern for crawl in `src/reference_import/crawl.rs`:
```rust
pub(crate) async fn fetch_crawl_manifest(
    config: &ImportConfig,
) -> Result<Vec<(String, String, String)>, ImportError>
//  Returns: vec of (topic-relative path, content, source_url)
```

This does the BFS crawl, fetches pages, extracts content, returns without DB writes.
Then refactor `import_crawl` to call this + the DB write loop.

- [ ] **Step 5: Verify existing import tests still pass**

Run the existing import tests to confirm the refactor didn't break anything:
```bash
cargo test --features live-tests reference_import_git -- --nocapture
cargo test --features live-tests reference_import_crawl -- --nocapture
```

Fix any regressions before proceeding.

- [ ] **Step 6: Implement `update_references` using the manifests**

Wire up the full algorithm from step 2 using `fetch_git_manifest` /
`fetch_crawl_manifest`, the diff logic, and the orphan handling.

For the orphan move:
- Create `references/{topic}/_orphaned/` directory
- Move the disk file there
- Update DB: `update_reference_path(db, ref_id, new_path, topic_id)` where `new_path`
  is `{topic}/_orphaned/{filename}`
- Print warning: `"Warning: {path} deleted upstream but cited by notes: {note_ids}. Moved to _orphaned/"`

- [ ] **Step 7: Run `just ci`**

- [ ] **Step 8: Commit**

```
feat: core reference update logic with diff and orphan protection
```

---

## Task 7: CLI `update` subcommand

Wire the update logic into the CLI.

**Files:**
- Modify: `src/cli/reference.rs` — add `Update` variant to `ReferenceCommand`

- [ ] **Step 1: Add `Update` subcommand**

```rust
#[derive(Debug, Subcommand)]
pub enum ReferenceCommand {
    Import { /* existing */ },
    Delete { /* existing */ },
    /// Update references for a topic from its original source
    Update {
        /// Topic name (e.g. "dioxus/docs")
        #[arg(long)]
        topic: String,
        /// Override git ref (branch or tag) for this update
        #[arg(long = "ref")]
        git_ref: Option<String>,
    },
}
```

- [ ] **Step 2: Implement the `execute` match arm**

In `execute()`, add the `Update` handler:
```rust
ReferenceCommand::Update { topic, git_ref } => {
    let result = crate::reference_import::update_references(
        &db, workspace, &topic, git_ref.as_deref(),
    ).await?;
    print_update_result(&topic, result);
    Ok(())
}
```

Add `print_update_result` that shows created/updated/deleted/orphaned/unchanged counts
and version change (old → new).

- [ ] **Step 3: Run `just ci`**

- [ ] **Step 4: Commit**

```
feat: add `ghost reference update` CLI command
```

---

## Task 8: Update the reference-import skill

The skill file teaches the AI when and how to use the CLI. Add documentation for the
update command.

**Files:**
- Modify: `assets/skills/reference-import/skill.md`

- [ ] **Step 1: Add update section to skill**

After the "Cleanup" section, add:

```markdown
## Updating References

When the OPERATOR asks to refresh or update existing reference material, or when you
notice imported docs may be stale (e.g. a library released a new version):

```
ghost reference update --topic <name> [--ref <tag-or-branch>]
```

This re-fetches from the original source and applies changes:
- New files are added
- Changed files are updated (content + embeddings)
- Files deleted upstream are removed — unless cited by notes, in which case they're
  moved to `_orphaned/` with a warning

Examples:
```json
{ "command": "ghost reference update --topic dioxus/docs", "background": true }
{ "command": "ghost reference update --topic dioxus/docs --ref v0.6", "background": true }
```
```

Also update the "CLI Commands" section to include `ghost reference update`.

- [ ] **Step 2: Commit**

```
docs: document reference update command in skill
```

---

## Task 9: Live test — git import → update → verify diff

End-to-end test that imports a git repo, then simulates an update and verifies the diff
behavior. Read the `/testing` skill before writing this test.

**Files:**
- Create: `tests/reference_update_git.rs`

- [ ] **Step 1: Write the test**

Pattern: use the same Dioxus docsite repo as `reference_import_git.rs`. The test:

1. Import with `import_git` (same as existing test)
2. Record the initial `version_ref` and reference count
3. Manually modify one reference's `file_hash` in the DB to simulate upstream change
   (set it to `"stale"` so the real content will differ)
4. Manually insert a fake reference with path `{topic}/deleted-upstream.md` that won't
   exist in the fresh clone — to test deletion
5. Create a note that cites the fake reference (via `create_cited_edge`) — to test
   orphan protection
6. Run `update_references(db, workspace, topic, None)`
7. Assert:
   - The modified reference was updated (hash changed from `"stale"` to real hash)
   - The fake reference was moved to `_orphaned/` (because it's cited)
   - `UpdateResult.orphaned == 1`
   - `UpdateResult.updated >= 1`
   - No data loss (all originally-created refs still exist or were updated)

- [ ] **Step 2: Run the test**

```bash
cargo test --features live-tests reference_update_git -- --nocapture
```

- [ ] **Step 3: Fix any failures — fix the code, not the test**

- [ ] **Step 4: Commit**

```
test: live test for reference update with diff and orphan protection
```

---

## Task 10: Final `just ci` and cleanup

- [ ] **Step 1: Run `just ci`** — fix all warnings, clippy lints, formatting
- [ ] **Step 2: Verify `_import.toml` backward compatibility** — old TOML files without
  the new fields should still parse (the `#[serde(default)]` annotations handle this)
- [ ] **Step 3: Commit any remaining fixes**

```
chore: ci cleanup for reference update feature
```
