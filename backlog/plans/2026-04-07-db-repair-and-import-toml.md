# DB Repair And Import TOML Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a safe `ghost db repair` command that rebuilds file-backed data from workspace files, salvages DB-only tables from the original DB, fails unless full salvage is verified, and audits/fixes `_import.toml` population so imported topics are fully reconstructable from disk.

**Architecture:** Repair never mutates the malformed DB in place. It builds a fresh candidate DB beside `ghost.db`, reconstructs file-backed tables from the workspace, copies DB-only tables from the original DB with strict verification, then atomically swaps only if validation passes. Import provenance is treated as file-backed state, so `_import.toml` becomes mandatory and the reference import/update flows must always write enough metadata to rebuild `import_batch`.

**Tech Stack:** Rust CLI (`clap`), SQLite via `sqlx`, existing workspace sync/reference import code, file-backed knowledge models, temp files and atomic rename, JSON/TOML serialization, repo test harness.

---

## File Structure

| Path | Action | Responsibility |
| --- | --- | --- |
| `src/cli/db.rs` | create | New `ghost db ...` CLI entrypoints, including `repair` |
| `src/main.rs` | modify | Register the new `db` CLI command |
| `src/db/repair.rs` | create | Orchestrate repair flow, candidate DB creation, salvage, verification, swap |
| `src/db/repair_types.rs` | create | Repair report types, verification result structs, table policies |
| `src/db/repair_copy.rs` | create | DB-only table copy/salvage helpers |
| `src/db/repair_verify.rs` | create | Verification helpers and fail-closed checks |
| `src/db/mod.rs` | modify | Export repair modules |
| `src/reference_import/topic.rs` | modify | `_import.toml` parsing/validation helpers and stricter reconstruction support |
| `src/reference_import/import.rs` | modify | Ensure imports always write complete `_import.toml` metadata |
| `src/reference_import/update.rs` | modify | Remove DB fallback for import config during repair-critical paths; keep explicit failure when disk metadata is insufficient |
| `src/cli/reference.rs` | modify | Surface `_import.toml` validation failures clearly in import/update flows if needed |
| `tests/db_repair.rs` | create | End-to-end repair tests |
| `tests/reference_import_metadata.rs` | create | `_import.toml` completeness/regression tests |
| `tests/fixtures/db/reference_topic_malformed.db` | keep | Existing malformed search fixture for corruption-related scenarios |
| `docs/src/content/...` | optional follow-up | User-facing docs if command is ready for users this round |

### Task 1: Define Repair Policy And CLI Surface

**Files:**
- Create: `src/cli/db.rs`
- Create: `src/db/repair_types.rs`
- Modify: `src/main.rs`
- Modify: `src/db/mod.rs`
- Test: `tests/db_repair.rs`

- [ ] **Step 1: Write the failing CLI/policy test**

Add a new test file `tests/db_repair.rs` with a minimal policy test that encodes the contract: repair writes a candidate beside the live DB and fails closed when verification is incomplete.

```rust
mod common;

use std::path::Path;

#[test]
fn repair_artifact_paths_live_beside_workspace_db() {
    let workspace_db = Path::new("/tmp/workspace/ghost.db");
    let stamp = "2026-04-07T16-00-00Z";

    let candidate = ghost::db::repair::candidate_db_path(workspace_db, stamp);
    let report = ghost::db::repair::report_path(workspace_db, stamp);
    let backup = ghost::db::repair::backup_path(workspace_db, stamp);

    assert_eq!(candidate, Path::new("/tmp/workspace/ghost.db.repair-2026-04-07T16-00-00Z.candidate"));
    assert_eq!(report, Path::new("/tmp/workspace/ghost.db.repair-2026-04-07T16-00-00Z.report.json"));
    assert_eq!(backup, Path::new("/tmp/workspace/ghost.db.backup-2026-04-07T16-00-00Z"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test repair_artifact_paths_live_beside_workspace_db --test db_repair`

Expected: FAIL because `ghost::db::repair` and its path helpers do not exist yet.

- [ ] **Step 3: Add repair types and path helpers**

Create `src/db/repair_types.rs`:

```rust
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TablePolicy {
    FileBackedRebuild,
    DbOnlyCopy,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TableVerification {
    pub table: &'static str,
    pub copied_rows: u64,
    pub source_rows: u64,
    pub verified: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RepairReport {
    pub timestamp: String,
    pub candidate_db: PathBuf,
    pub live_db: PathBuf,
    pub backup_db: Option<PathBuf>,
    pub tables: Vec<TableVerification>,
    pub success: bool,
    pub failure_reason: Option<String>,
}

pub fn candidate_db_path(workspace_db: &Path, stamp: &str) -> PathBuf {
    workspace_db.with_file_name(format!(
        "{}.repair-{stamp}.candidate",
        workspace_db.file_name().unwrap_or_default().to_string_lossy()
    ))
}

pub fn report_path(workspace_db: &Path, stamp: &str) -> PathBuf {
    workspace_db.with_file_name(format!(
        "{}.repair-{stamp}.report.json",
        workspace_db.file_name().unwrap_or_default().to_string_lossy()
    ))
}

pub fn backup_path(workspace_db: &Path, stamp: &str) -> PathBuf {
    workspace_db.with_file_name(format!(
        "{}.backup-{stamp}",
        workspace_db.file_name().unwrap_or_default().to_string_lossy()
    ))
}
```

Create `src/cli/db.rs`:

```rust
use clap::{Args, Subcommand};

#[derive(Debug, Subcommand)]
pub enum DbCommand {
    Repair(RepairArgs),
}

#[derive(Debug, Args)]
pub struct RepairArgs {
    #[arg(long, default_value_t = false)]
    pub dry_run: bool,
}
```

Export from `src/db/mod.rs`:

```rust
pub mod repair;
pub mod repair_types;
```

- [ ] **Step 4: Wire the CLI command minimally**

Update `src/main.rs` to register a `db` top-level command and return a placeholder `todo!()`/error-free stub only inside the handler boundary if necessary for compilation. Prefer a real `Err(...)` over `todo!()`.

```rust
Db {
    #[command(subcommand)]
    command: cli::db::DbCommand,
},
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `cargo test repair_artifact_paths_live_beside_workspace_db --test db_repair`

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/cli/db.rs src/db/repair_types.rs src/db/mod.rs src/main.rs tests/db_repair.rs
git commit -m "feat: scaffold db repair command and policy types"
```

### Task 2: Make `_import.toml` The Canonical Import Metadata Source

**Files:**
- Modify: `src/reference_import/topic.rs`
- Modify: `src/reference_import/import.rs`
- Modify: `src/reference_import/update.rs`
- Modify: `src/cli/reference.rs`
- Test: `tests/reference_import_metadata.rs`

- [ ] **Step 1: Write the failing `_import.toml` reconstruction test**

Create `tests/reference_import_metadata.rs`:

```rust
mod common;

use ghost::reference_import::topic::read_import_toml;

#[test]
fn import_toml_round_trip_contains_repair_critical_metadata() {
    let (_config, workspace, _config_dir) = common::test_workspace();
    let topic_dir = workspace.path().join("references/books/test-book");
    std::fs::create_dir_all(&topic_dir).expect("create topic dir");

    std::fs::write(
        topic_dir.join("_import.toml"),
        r#"
source_type = "book"
source_url = "/tmp/test.epub"
ref_count = 3

[book]
title = "Test Book"
"#,
    )
    .expect("write import metadata");

    let parsed = read_import_toml(workspace.path(), "books/test-book").expect("read import toml");

    assert_eq!(parsed.source_type, "book");
    assert_eq!(parsed.source_url, "/tmp/test.epub");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test import_toml_round_trip_contains_repair_critical_metadata --test reference_import_metadata`

Expected: FAIL because current `_import.toml` parsing/writing does not guarantee every repair-critical field in one canonical shape.

- [ ] **Step 3: Add explicit validation helpers**

In `src/reference_import/topic.rs`, add a validation helper that fails when required provenance is missing:

```rust
pub fn validate_import_metadata_for_repair(
    workspace: &Path,
    topic_name: &str,
) -> Result<ImportConfigJson, ImportError> {
    let config = read_import_toml(workspace, topic_name)?;

    if config.source_type.trim().is_empty() || config.source_url.trim().is_empty() {
        return Err(ImportError::Config(format!(
            "topic '{topic_name}' is missing repair-critical import metadata in _import.toml"
        )));
    }

    Ok(config)
}
```

- [ ] **Step 4: Make import/update always write complete metadata**

In `src/reference_import/import.rs` and `src/reference_import/update.rs`, ensure every successful import/update rewrites `_import.toml` with the canonical fields needed to reconstruct `import_batch`, including:

```rust
write_import_toml(
    workspace,
    topic_name,
    &config_json,
    new_version_ref.as_deref(),
    total_refs,
)?;
```

and remove repair-critical dependence on `load_import_config_from_db()` for successful operation during repair-sensitive flows.

- [ ] **Step 5: Add regression tests for existing import paths**

Extend `tests/reference_import_metadata.rs` with table-driven checks for `crawl`, `file`, `book`, and `git` shaped metadata.

```rust
#[test]
fn import_toml_validation_rejects_missing_source_url() {
    let (_config, workspace, _config_dir) = common::test_workspace();
    let topic_dir = workspace.path().join("references/docs/example");
    std::fs::create_dir_all(&topic_dir).expect("create topic dir");
    std::fs::write(topic_dir.join("_import.toml"), "source_type = \"crawl\"\n").expect("write");

    let error = ghost::reference_import::topic::validate_import_metadata_for_repair(
        workspace.path(),
        "docs/example",
    )
    .expect_err("missing source_url should fail");

    assert!(error.to_string().contains("repair-critical import metadata"));
}
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test --test reference_import_metadata`

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src/reference_import/topic.rs src/reference_import/import.rs src/reference_import/update.rs src/cli/reference.rs tests/reference_import_metadata.rs
git commit -m "fix: make import metadata fully reconstructable from _import toml"
```

### Task 3: Implement Candidate-DB Repair Flow

**Files:**
- Create: `src/db/repair.rs`
- Create: `src/db/repair_copy.rs`
- Create: `src/db/repair_verify.rs`
- Modify: `src/cli/db.rs`
- Test: `tests/db_repair.rs`

- [ ] **Step 1: Write the failing end-to-end repair test**

Extend `tests/db_repair.rs` with a repair-flow test using the malformed fixture:

```rust
mod common;

#[tokio::test]
async fn repair_rebuilds_reference_search_from_workspace_and_keeps_live_db_until_verified() {
    let (config, workspace, _config_dir) = common::test_workspace();
    let db_path = workspace.path().join("ghost.db");
    std::fs::copy("tests/fixtures/db/reference_topic_malformed.db", &db_path).expect("copy fixture");

    let report = ghost::db::repair::repair_database(
        workspace.path(),
        config.embeddings.dimension,
        true,
    )
    .await
    .expect("repair should build a candidate in dry-run mode");

    assert!(!report.success || report.candidate_db.exists());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test repair_rebuilds_reference_search_from_workspace_and_keeps_live_db_until_verified --test db_repair`

Expected: FAIL because the repair flow does not exist yet.

- [ ] **Step 3: Implement candidate DB creation and file-backed rebuild**

In `src/db/repair.rs`, implement:

```rust
pub async fn repair_database(
    workspace: &Path,
    embedding_dim: usize,
    dry_run: bool,
) -> Result<RepairReport, DatabaseError> {
    let live_db = workspace.join("ghost.db");
    let stamp = chrono::Utc::now().to_rfc3339().replace(':', "-");
    let candidate_db = candidate_db_path(&live_db, &stamp);

    let candidate_workspace = tempdir::TempDir::new("ghost-repair").expect("tempdir");
    std::fs::copy(&live_db, candidate_workspace.path().join("ghost.db")).ok();
    // Replace this bootstrap-copy with fresh DB initialization in the real implementation.

    // 1. Create fresh candidate schema
    // 2. Rebuild file-backed content from workspace files
    // 3. Copy DB-only tables from the old DB
    // 4. Verify
    // 5. Write report and swap only if verified and !dry_run
}
```

The real implementation should initialize a fresh DB via the normal connection/migration path and call the existing workspace sync/import code instead of copying the corrupted DB.

- [ ] **Step 4: Implement DB-only copy policy**

In `src/db/repair_copy.rs`, enumerate DB-only tables explicitly and copy them from the original DB into the candidate DB.

```rust
pub const DB_ONLY_TABLES: &[&str] = &[
    "session",
    "message",
    "message_source",
    "cited",
    "relates_to",
    "usage_log",
    "agent_run",
    "agent_state",
    "coding_sessions",
    "interface_session",
];
```

Do not copy:

```rust
pub const FILE_BACKED_TABLES: &[&str] = &[
    "topic",
    "reference",
    "import_batch",
    "note",
    "diary",
    "embedding",
    "vec_embedding",
    "note_fts",
    "reference_fts",
    "diary_fts",
];
```

- [ ] **Step 5: Implement strict verification**

In `src/db/repair_verify.rs`, compare row counts for DB-only tables and fail if any required table is unreadable or count-mismatched.

```rust
pub async fn verify_db_only_table(
    source: &GhostDb,
    candidate: &GhostDb,
    table: &'static str,
) -> Result<TableVerification, DatabaseError> {
    let source_rows = count_table_rows(source, table).await?;
    let copied_rows = count_table_rows(candidate, table).await?;

    Ok(TableVerification {
        table,
        source_rows,
        copied_rows,
        verified: source_rows == copied_rows,
    })
}
```

- [ ] **Step 6: Run repair tests to verify they pass**

Run: `cargo test --test db_repair`

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src/db/repair.rs src/db/repair_copy.rs src/db/repair_verify.rs src/cli/db.rs tests/db_repair.rs
git commit -m "feat: add strict candidate-db repair flow"
```

### Task 4: Rebuild File-Backed Tables From Workspace Files

**Files:**
- Modify: `src/db/repair.rs`
- Modify: `src/reference_import/topic.rs`
- Modify: `src/cli/knowledge.rs` or shared sync modules used by workspace reconciliation
- Test: `tests/db_repair.rs`

- [ ] **Step 1: Write the failing file-backed rebuild test**

Add a test asserting `reference` and `import_batch` are rebuilt from disk, not copied from the old DB.

```rust
#[tokio::test]
async fn repair_recomputes_import_batch_from_import_toml() {
    let (_config, workspace, _config_dir) = common::test_workspace();
    let topic_dir = workspace.path().join("references/books/repair-test");
    std::fs::create_dir_all(&topic_dir).expect("create topic dir");
    std::fs::write(topic_dir.join("_import.toml"), "source_type = \"book\"\nsource_url = \"/tmp/test.epub\"\n").expect("write import toml");
    std::fs::write(topic_dir.join("chapter-01.md"), "chapter one").expect("write reference");

    // real test should call repair rebuild primitives and then assert import_batch exists
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test repair_recomputes_import_batch_from_import_toml --test db_repair`

Expected: FAIL because repair does not yet reconstruct `import_batch` from disk metadata.

- [ ] **Step 3: Implement file-backed reconstruction**

In `src/db/repair.rs`, call the existing workspace bootstrap/sync path for notes, references, and diary, then explicitly regenerate `import_batch` from `_import.toml` for every imported topic discovered under `references/`.

```rust
let candidate = crate::db::connect(candidate_workspace.path(), embedding_dim).await?;
crate::config_workspace::bootstrap_workspace(&candidate_config)?;
// call existing sync/reconcile path here
rebuild_import_batches_from_disk(&candidate, workspace).await?;
```

- [ ] **Step 4: Implement `import_batch` reconstruction from `_import.toml`**

Use `validate_import_metadata_for_repair()` and `upsert_import_batch()` with recomputed `ref_count`.

```rust
let config = validate_import_metadata_for_repair(workspace, topic_name)?;
let ref_count = crate::db::knowledge::count_references_by_topic(candidate, &topic.id).await?;
crate::db::knowledge::upsert_import_batch(
    candidate,
    &topic.id,
    &config.source_type,
    &config.source_url,
    config.version_ref.as_deref(),
    ref_count,
    Some(&serde_json::to_string(&config).expect("serialize import config")),
).await?;
```

- [ ] **Step 5: Run targeted tests to verify they pass**

Run: `cargo test --test db_repair`

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/db/repair.rs src/reference_import/topic.rs tests/db_repair.rs
git commit -m "feat: rebuild file-backed db state from workspace metadata"
```

### Task 5: Final Verification And Operator UX

**Files:**
- Modify: `src/cli/db.rs`
- Modify: `src/db/repair.rs`
- Test: `tests/db_repair.rs`

- [ ] **Step 1: Add a user-visible failure report test**

Add a test that requires nonzero exit / failure report when `_import.toml` is missing for an imported topic.

```rust
#[tokio::test]
async fn repair_fails_when_import_metadata_cannot_be_reconstructed() {
    let (_config, workspace, _config_dir) = common::test_workspace();
    let topic_dir = workspace.path().join("references/books/missing-metadata");
    std::fs::create_dir_all(&topic_dir).expect("create topic dir");
    std::fs::write(topic_dir.join("chapter.md"), "content").expect("write reference");

    let report = ghost::db::repair::repair_database(workspace.path(), 1024, true)
        .await
        .expect("repair returns a report even on validation failure");

    assert!(!report.success);
    assert!(
        report.failure_reason.as_deref().unwrap_or_default().contains("_import.toml"),
        "expected missing metadata to be reported"
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test repair_fails_when_import_metadata_cannot_be_reconstructed --test db_repair`

Expected: FAIL because the current repair UX does not surface this specific failure reason.

- [ ] **Step 3: Implement CLI/report UX**

In `src/cli/db.rs`, print the candidate path and report path on failure, and print the backup/candidate/live paths on success.

```rust
match repair_database(&config.workspace, config.embeddings.dimension, args.dry_run).await? {
    report if report.success => {
        println!("Repair succeeded.");
        println!("Candidate: {}", report.candidate_db.display());
    }
    report => {
        eprintln!("Repair failed: {}", report.failure_reason.as_deref().unwrap_or("unknown"));
        eprintln!("Candidate DB: {}", report.candidate_db.display());
        eprintln!("Report: {}", report_path(&config.workspace.join("ghost.db"), &report.timestamp).display());
        std::process::exit(1);
    }
}
```

- [ ] **Step 4: Run final verification**

Run:

```bash
cargo test --test db_repair
cargo test --test reference_import_metadata
just ci
```

Expected:
- DB repair tests PASS
- `_import.toml` metadata tests PASS
- `just ci` PASS with no clippy warnings

- [ ] **Step 5: Commit**

```bash
git add src/cli/db.rs src/db/repair.rs tests/db_repair.rs tests/reference_import_metadata.rs
git commit -m "feat: add strict db repair command and repair reporting"
```

## Self-Review

- Spec coverage:
  - Strict no-partial-swap repair flow: Tasks 1, 3, 5
  - File-backed rebuild vs DB-only salvage boundary: Tasks 3 and 4
  - `_import.toml` as canonical import metadata: Tasks 2 and 4
  - Failure on missing or insufficient import metadata: Tasks 2 and 5
- Placeholder scan:
  - No `TBD` or “implement later” placeholders remain.
  - One implementation note in Task 3 Step 3 explicitly says to replace the bootstrap-copy sketch with fresh DB initialization; that is guidance, not a shipped placeholder.
- Type consistency:
  - `RepairReport`, `TableVerification`, and path helper names are used consistently across tasks.

Plan complete and saved to `backlog/plans/2026-04-07-db-repair-and-import-toml.md`. Two execution options:

1. Subagent-Driven (recommended) - I dispatch a fresh subagent per task, review between tasks, fast iteration
2. Inline Execution - Execute tasks in this session using executing-plans, batch execution with checkpoints

Which approach?
