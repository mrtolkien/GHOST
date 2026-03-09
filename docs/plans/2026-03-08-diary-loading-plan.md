# Diary Loading Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this
> plan task-by-task.

**Goal:** Load the last 2 diary entries into chat context at session start and
post-compaction for cross-session continuity.

**Architecture:** Read diary files directly from disk (source of truth) in
`build_ghost_diary`. Split `knowledge/files.rs` into per-type modules first. Clean up
dead `JobPromptContext` code.

**Tech Stack:** Rust, std::fs, chrono (existing deps only)

**Design doc:** `docs/plans/2026-03-08-diary-loading-design.md`

**Skill references:** Read `@testing` before writing tests. Run `just ci` after each
task.

---

### Task 1: Extract `knowledge/notes.rs` from `files.rs`

Pure refactor — move note functions to their own module.

**Files:**

- Create: `src/knowledge/notes.rs`
- Modify: `src/knowledge/files.rs` (remove note functions)
- Modify: `src/knowledge/mod.rs` (add `mod notes`, update re-exports)

**Step 1: Create `src/knowledge/notes.rs`**

Move these functions from `files.rs` into `notes.rs`:

- `note_path`
- `subfolder_from_tags`
- `read_note`
- `write_note`
- `note_relative_path`
- `ensure_index_notes`

Move these tests:

- `write_then_read_roundtrip`
- `write_note_no_tags_goes_flat`
- `ensure_index_notes_creates_hierarchy`
- `list_notes_finds_files_recursively`
- `list_notes_empty_dir`
- `note_path_construction`
- `note_path_with_subfolder`
- `subfolder_from_tags_extracts_first`

The file needs these imports:

```rust
use std::path::{Path, PathBuf};

use super::error::KnowledgeError;
use super::files::collect_md_files_recursive;
use super::parser::{parse_note, serialize_note};
use super::types::{NoteFrontMatter, ParsedNote};
```

Also move `list_notes` here (it uses `collect_md_files_recursive` from files.rs). Make
`collect_md_files_recursive` in `files.rs` `pub(super)` so `notes.rs` can use it.

**Step 2: Update `src/knowledge/mod.rs`**

```rust
mod diary; // (added in task 2)
mod error;
mod files;
mod notes;
mod parser;
pub mod reconcile;
mod types;

pub use error::KnowledgeError;
pub use files::{list_references, reference_path};
pub use notes::{
    ensure_index_notes, list_notes, note_path, note_relative_path, read_note,
    subfolder_from_tags, write_note,
};
// diary re-exports added in task 2
pub use parser::{extract_wiki_links, parse_note, serialize_note, slug_from_title};
pub use types::{KnowledgeKind, NoteFrontMatter, ParsedNote, WikiLink};
```

**Step 3: Remove moved functions and tests from `files.rs`**

Remove from `files.rs`:

- `note_path` (lines 8-14)
- `subfolder_from_tags` (lines 19-21)
- `read_note` (lines 35-42)
- `write_note` (lines 44-67)
- `note_relative_path` (lines 72-77)
- `ensure_index_notes` (lines 86-137)
- `list_notes` (lines 174-183)
- All note-related tests
- The `use super::parser` and `use super::types` imports (no longer needed in files.rs)

Make `collect_md_files_recursive` `pub(super)` (was `fn`). Make `list_md_files`
`pub(super)` (diary.rs will need it in task 2).

**Step 4: Run `just ci` — all tests pass, no clippy warnings**

**Step 5: Commit**

```
git add src/knowledge/notes.rs src/knowledge/files.rs src/knowledge/mod.rs
git commit -m "refactor: extract knowledge/notes.rs from files.rs"
```

---

### Task 2: Extract `knowledge/diary.rs` from `files.rs`

Pure refactor — move diary functions to their own module.

**Files:**

- Create: `src/knowledge/diary.rs`
- Modify: `src/knowledge/files.rs` (remove diary functions)
- Modify: `src/knowledge/mod.rs` (add `mod diary`, update re-exports)

**Step 1: Create `src/knowledge/diary.rs`**

Move these functions from `files.rs`:

- `diary_path`
- `load_diary_today`
- `read_diary`
- `write_diary`
- `list_diary_entries`

Move these tests:

- `diary_write_and_read`

Imports:

```rust
use std::path::{Path, PathBuf};

use super::error::KnowledgeError;
use super::files::list_md_files;
```

Note: `load_diary_today` uses `chrono::Utc` — keep that import.

**Step 2: Update `src/knowledge/mod.rs` re-exports**

Add:

```rust
pub use diary::{
    diary_path, list_diary_entries, load_diary_today, read_diary, write_diary,
};
```

Remove diary items from the `files` re-export line.

**Step 3: Remove moved functions and tests from `files.rs`**

Remove `diary_path`, `load_diary_today`, `read_diary`, `write_diary`,
`list_diary_entries`, and `diary_write_and_read` test.

After this, `files.rs` should only contain:

- `reference_path`
- `list_references`
- `list_md_files` (pub(super))
- `collect_md_files_recursive` (pub(super))
- `list_references_recursive` test
- `reference_path_construction` test

**Step 4: Run `just ci` — all tests pass**

**Step 5: Commit**

```
git add src/knowledge/diary.rs src/knowledge/files.rs src/knowledge/mod.rs
git commit -m "refactor: extract knowledge/diary.rs from files.rs"
```

---

### Task 3: Add `load_recent_diary` and wire into prompt

The feature itself — read last 2 diary entries and inject into system prompt.

**Files:**

- Modify: `src/knowledge/diary.rs` (add `load_recent_diary`)
- Modify: `src/knowledge/mod.rs` (re-export `load_recent_diary`)
- Modify: `src/prompt/context.rs` (implement `build_ghost_diary`)
- Modify: `src/prompt/renderer.rs` (pass workspace to `build_ghost_diary`)

**Step 1: Write test for `load_recent_diary` in `diary.rs`**

```rust
#[test]
fn load_recent_diary_returns_last_n_entries() {
    let workspace = TempDir::new().unwrap();
    let diary_dir = workspace.path().join("diary");
    std::fs::create_dir_all(&diary_dir).unwrap();

    std::fs::write(diary_dir.join("2026-03-05.md"), "Day five.").unwrap();
    std::fs::write(diary_dir.join("2026-03-06.md"), "Day six.").unwrap();
    std::fs::write(diary_dir.join("2026-03-07.md"), "Day seven.").unwrap();
    std::fs::write(diary_dir.join("2026-03-08.md"), "").unwrap(); // empty, skipped

    let entries = load_recent_diary(workspace.path(), 2);
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].0, "2026-03-06");
    assert_eq!(entries[0].1, "Day six.");
    assert_eq!(entries[1].0, "2026-03-07");
    assert_eq!(entries[1].1, "Day seven.");
}

#[test]
fn load_recent_diary_empty_dir() {
    let workspace = TempDir::new().unwrap();
    let entries = load_recent_diary(workspace.path(), 2);
    assert!(entries.is_empty());
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p ghost knowledge::diary::tests::load_recent -- --nocapture` Expected:
FAIL — `load_recent_diary` doesn't exist yet.

**Step 3: Implement `load_recent_diary` in `diary.rs`**

```rust
/// Load the most recent `count` non-empty diary entries from disk.
/// Returns `(date, body)` pairs in chronological order.
#[must_use]
pub fn load_recent_diary(workspace: &Path, count: usize) -> Vec<(String, String)> {
    let paths = match list_diary_entries(workspace) {
        Ok(p) => p,
        Err(_) => return Vec::new(),
    };

    paths
        .iter()
        .rev()
        .filter_map(|path| {
            let date = path.file_stem()?.to_str()?.to_string();
            let body = std::fs::read_to_string(path).ok()?;
            if body.trim().is_empty() {
                return None;
            }
            Some((date, body))
        })
        .take(count)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}
```

**Step 4: Run tests to verify they pass**

**Step 5: Add re-export in `mod.rs`**

Add `load_recent_diary` to the diary re-export line.

**Step 6: Wire `build_ghost_diary` in `src/prompt/context.rs`**

Replace the stub:

```rust
/// Build recent diary entries for the system prompt.
#[tracing::instrument(skip_all, level = "debug", fields(workspace = %workspace.display()))]
pub fn build_ghost_diary(workspace: &Path) -> String {
    let entries = crate::knowledge::load_recent_diary(workspace, 2);
    if entries.is_empty() {
        return String::new();
    }

    let mut parts = vec!["## Diary\n".to_string()];
    for (date, body) in &entries {
        parts.push(format!("### {date}\n\n{body}"));
    }
    parts.join("\n")
}
```

**Step 7: Update call site in `src/prompt/renderer.rs:47`**

```rust
// Before:
let ghost_diary = context::build_ghost_diary();
// After:
let ghost_diary = context::build_ghost_diary(workspace);
```

**Step 8: Write test for `build_ghost_diary` in `context.rs`**

```rust
#[test]
fn build_ghost_diary_formats_recent_entries() {
    let dir = TempDir::new().unwrap();
    let diary_dir = dir.path().join("diary");
    fs::create_dir_all(&diary_dir).unwrap();
    fs::write(diary_dir.join("2026-03-07.md"), "Had a great chat.").unwrap();
    fs::write(diary_dir.join("2026-03-08.md"), "Built a feature.").unwrap();

    let result = build_ghost_diary(dir.path());
    assert!(result.contains("## Diary"));
    assert!(result.contains("### 2026-03-07"));
    assert!(result.contains("Had a great chat."));
    assert!(result.contains("### 2026-03-08"));
    assert!(result.contains("Built a feature."));
}

#[test]
fn build_ghost_diary_empty_when_no_entries() {
    let dir = TempDir::new().unwrap();
    let result = build_ghost_diary(dir.path());
    assert!(result.is_empty());
}
```

**Step 9: Run `just ci` — all tests pass**

**Step 10: Commit**

```
git add src/knowledge/diary.rs src/knowledge/mod.rs src/prompt/context.rs src/prompt/renderer.rs
git commit -m "feat: load last 2 diary entries into system prompt"
```

---

### Task 4: Delete dead `JobPromptContext` code

**Files:**

- Modify: `src/prompt/renderer.rs` (delete struct, method, test)
- Modify: `src/prompt/mod.rs` (remove re-export)

**Step 1: Delete from `renderer.rs`**

- Delete `JobPromptContext` struct (lines 18-26)
- Delete `render_job_prompt` method (lines 65-89)
- Delete `job_prompt_interpolates_provided_vars_and_blanks_missing` test (lines 139-171)

**Step 2: Remove re-export from `prompt/mod.rs`**

```rust
// Before:
pub use renderer::{JobPromptContext, PromptContext, PromptRenderer};
// After:
pub use renderer::{PromptContext, PromptRenderer};
```

**Step 3: Run `just ci` — all tests pass, no dead code warnings**

**Step 4: Commit**

```
git add src/prompt/renderer.rs src/prompt/mod.rs
git commit -m "refactor: remove dead JobPromptContext code"
```
