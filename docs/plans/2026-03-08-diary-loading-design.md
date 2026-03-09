# Diary Loading into Session Context

**Date**: 2026-03-08 **Spec**: `specs/1_diary.md` **Goal**: Load recent diary entries
into chat context at session start and post-compaction so GHOST has continuity across
sessions.

## Decision: File-based, sync reads

Diary files on disk (`<workspace>/diary/YYYY-MM-DD.md`) are the source of truth. We read
the last 2 files directly from disk rather than querying SQLite. This keeps
`render_system_prompt` sync and avoids threading a DB pool through the prompt layer.

## Design

### 1. File restructuring

Split `knowledge/files.rs` into per-type modules:

- **`knowledge/diary.rs`** — `diary_path`, `load_diary_today`, `read_diary`,
  `write_diary`, `list_diary_entries`, new `load_recent_diary`
- **`knowledge/notes.rs`** — `note_path`, `subfolder_from_tags`, `read_note`,
  `write_note`, `note_relative_path`, `ensure_index_notes`
- **`knowledge/files.rs`** — shared helpers (`list_md_files`,
  `collect_md_files_recursive`) + `reference_path`, `list_references` (references too
  small for own module)

`knowledge/mod.rs` re-exports stay the same. Tests move with their functions.

### 2. Diary loading

New function in `diary.rs`:

```rust
pub fn load_recent_diary(workspace: &Path, count: usize) -> Vec<(String, String)>
```

- Calls `list_diary_entries` (already sorted by filename = date)
- Takes last `count` entries
- Reads content, skips empty files
- Returns `(date, body)` pairs in chronological order

Wire into prompt layer:

- Change `build_ghost_diary()` to `build_ghost_diary(workspace: &Path)`
- Calls `load_recent_diary(workspace, 2)`
- Formats output as:

```markdown
## Diary

### 2026-03-07

<body>

### 2026-03-08

<body>
```

- Returns empty string if no entries (placeholder vanishes)

Update `render_system_prompt` to pass `workspace` (already available).

Works for both session start and post-compaction since both call `render_system_prompt`.

### 3. Cleanup dead code

Delete from `prompt/renderer.rs`:

- `JobPromptContext` struct
- `render_job_prompt` method
- Associated test

Remove `JobPromptContext` re-export from `prompt/mod.rs`.

This is dead code from the old job system — agents use Lua now.
