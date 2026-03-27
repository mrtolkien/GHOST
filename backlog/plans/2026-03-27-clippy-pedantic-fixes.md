# Clippy Pedantic Fixes & Lint Configuration

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or superpowers:executing-plans
> to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix production-relevant clippy::pedantic warnings, refactor
`too_many_arguments` functions into struct-based APIs, and configure project-wide clippy
lints.

**Architecture:** Three workstreams — (C) add clippy lint config so new code is checked
from the start, (A) fix 6 categories of potential runtime bugs (casts, subtraction,
wildcard match, unused async), (B) introduce parameter structs to eliminate
`too_many_arguments` across 11 functions and remove the global allow.

**Tech Stack:** Rust, clippy, Cargo.toml `[lints]` section, `clippy.toml`

---

## File Map

**New files:**

- `clippy.toml` — clippy threshold configuration

**Modified files (Part C — lint config):**

- `Cargo.toml` — add `[lints.clippy]` section
- `justfile` — no changes needed (bare `cargo clippy` picks up `Cargo.toml` lints
  automatically)

**Modified files (Part A — bug fixes):**

- `src/interfaces/discord/ui_events.rs` — f64→u64 cast safety
- `src/reference_import/crawl.rs` — i64→usize sign loss
- `src/reference_import/file.rs` — i64→usize sign loss
- `src/reference_import/git.rs` — i64→usize sign loss
- `src/reference_import/update.rs` — i64→usize sign loss + wildcard match
- `src/tools/browser.rs` — u64→u32 truncation (4 sites)
- `src/web/browser/accessibility.rs` — u64→u32 truncation (1 site)
- `src/web/search.rs` — unchecked Duration subtraction
- `src/cli/agent.rs` — remove unused async
- `src/cli/reset.rs` — remove unused async
- `src/cli/services.rs` — remove unused async
- `src/cli/skills.rs` — remove unused async
- `src/cli/status.rs` — remove unused async
- `src/tools/browser.rs` — remove unused async (execute_tabs)
- `src/web/browser/manager.rs` — remove unused async (2 functions)
- `src/main.rs` — update call sites if they `.await` on de-async'd functions

**Modified files (Part B — struct refactoring):**

- `src/db/knowledge/crud.rs` — add `NoteRecord` struct, refactor `create_note_full` +
  `update_note`
- `src/db/sessions.rs` — add `MessagePayload` struct, refactor both
  `create_message_with_*` functions
- `src/tools/note_write.rs` — use `NoteRecord` in `create_note` + `update_note`
- `src/cli/note.rs` — use `NoteRecord` in `create_note` + `update_note`
- `src/cli/knowledge.rs` — update call sites
- `src/daemon/watcher.rs` — update call sites
- `src/knowledge/reconcile.rs` — update call sites
- `src/chat/session.rs` — use `MessagePayload`; add `ToolLoopContext`; update
  `run_agent_with_history`
- `src/chat/tool_loop.rs` — use `ToolLoopContext` in `run_tool_loop`
- `src/web/browser/accessibility.rs` — add `RenderState` struct, refactor `render_node`
- `src/lib.rs` — remove `#![allow(clippy::too_many_arguments)]`
- `tests/knowledge.rs` — update `create_note_full` / `update_note` call sites
- `tests/embeddings.rs` — update `create_note_full` call sites
- `tests/embedding_live.rs` — update `create_note_full` call site
- `tests/database.rs` — update `create_message_with_metadata` call sites
- `tests/chat_orchestration.rs` — update `create_message_with_metadata` call sites
- `tests/common.rs` — update `create_message_with_metadata` call site
- `tests/providers/out_of_sync_live.rs` — update call site
- `tests/providers/message_adjacency_live.rs` — update call sites

---

## Task 1: Clippy Lint Configuration

**Files:**

- Create: `clippy.toml`
- Modify: `Cargo.toml` (append `[lints.clippy]` section at end)

- [ ] **Step 1: Create `clippy.toml`**

```toml
# Async: warn when a Future exceeds 4 KiB (default is 16 KiB).
future-size-threshold = 4096

# Complexity thresholds.
cognitive-complexity-threshold = 25
too-many-lines-threshold = 100
type-complexity-threshold = 250

too-many-arguments-threshold = 7

# Keep Result<_, E> variants lean for efficient propagation.
large-error-threshold = 128

# Tests: allow unwrap/expect/panic (CLAUDE.md says "tests fine").
allow-unwrap-in-tests = true
allow-expect-in-tests = true
allow-dbg-in-tests = true
allow-print-in-tests = true
```

- [ ] **Step 2: Add `[lints.clippy]` to `Cargo.toml`**

Append after the `[dev-dependencies]` section:

```toml
[lints.clippy]
# Correctness — catches real bugs
await_holding_lock = "warn"
await_holding_refcell_ref = "warn"
debug_assert_with_mut_call = "warn"
path_buf_push_overwrite = "warn"

# Robustness — prevents future bugs
manual_let_else = "warn"
redundant_else = "warn"
match_same_arms = "warn"
dbg_macro = "warn"
todo = "warn"
panic_in_result_fn = "warn"
allow_attributes_without_reason = "warn"

# Performance — unnecessary allocations/copies
cloned_instead_of_copied = "warn"
inefficient_to_string = "warn"
implicit_clone = "warn"
large_futures = "warn"
redundant_closure_for_method_calls = "warn"
flat_map_option = "warn"

# Maintenance — keeps code clean
uninlined_format_args = "warn"
semicolon_if_nothing_returned = "warn"
enum_glob_use = "warn"
default_trait_access = "warn"
single_char_pattern = "warn"
```

- [ ] **Step 3: Run `just clippy` and verify it compiles**

Run: `just clippy` Expected: Compiles successfully. New warnings may appear — that's
fine (they're `warn`, not `deny`). No errors.

- [ ] **Step 4: Commit**

```bash
git add clippy.toml Cargo.toml
git commit -m "chore: add clippy lint configuration

Enable targeted clippy lints for correctness (await_holding_lock,
debug_assert_with_mut_call), robustness (match_same_arms, dbg_macro,
todo, panic_in_result_fn), performance (implicit_clone, large_futures),
and maintenance (uninlined_format_args, semicolon_if_nothing_returned).

Thresholds in clippy.toml: future-size 4KiB, too-many-args 7,
too-many-lines 100."
```

---

## Task 2: Fix f64→u64 Sign Loss in Discord UI Events

**Files:**

- Modify: `src/interfaces/discord/ui_events.rs:219-220,256-257`

- [ ] **Step 1: Fix `format_run_summary` duration casting (line 218-221)**

Replace:

```rust
    let secs = metadata.duration.as_secs_f64();
    let duration = if secs >= 60.0 {
        let mins = secs as u64 / 60;
        let remaining = secs as u64 % 60;
```

With:

```rust
    let secs = metadata.duration.as_secs_f64();
    let total_secs = secs.max(0.0) as u64;
    let duration = if total_secs >= 60 {
        let mins = total_secs / 60;
        let remaining = total_secs % 60;
```

- [ ] **Step 2: Fix `format_agent_summary` duration casting (line 254-258)**

Apply the same pattern:

```rust
    let secs = metadata.duration.as_secs_f64();
    let total_secs = secs.max(0.0) as u64;
    let duration = if total_secs >= 60 {
        let mins = total_secs / 60;
        let remaining = total_secs % 60;
```

- [ ] **Step 3: Run `just ci`**

Run: `just ci` Expected: PASS — all checks green.

- [ ] **Step 4: Commit**

```bash
git add src/interfaces/discord/ui_events.rs
git commit -m "fix: clamp duration to zero before f64→u64 cast

Negative secs (clock skew) would wrap to u64::MAX, producing
nonsensical duration strings."
```

---

## Task 3: Fix i64→usize Sign Loss in Reference Imports

All four reference import modules use the same pattern: `count_references_by_topic()`
returns `i64` (SQLite), cast directly to `usize`.

**Files:**

- Modify: `src/reference_import/crawl.rs:175`
- Modify: `src/reference_import/file.rs:144`
- Modify: `src/reference_import/git.rs:168`
- Modify: `src/reference_import/update.rs:135`

- [ ] **Step 1: Fix `crawl.rs:175`**

Replace:

```rust
    let total_refs = db::knowledge::count_references_by_topic(db, &topic_id).await? as usize;
```

With:

```rust
    let total_refs = usize::try_from(
        db::knowledge::count_references_by_topic(db, &topic_id).await?.max(0),
    )
    .unwrap_or(0);
```

- [ ] **Step 2: Fix `file.rs:144`**

Same pattern — replace `as usize` with:

```rust
    let total_refs = usize::try_from(
        db::knowledge::count_references_by_topic(db, &topic_id).await?.max(0),
    )
    .unwrap_or(0);
```

- [ ] **Step 3: Fix `git.rs:168`**

Same pattern.

- [ ] **Step 4: Fix `update.rs:135`**

Same pattern — note this one uses `topic_id` without `&`:

```rust
    let total_refs = usize::try_from(
        db::knowledge::count_references_by_topic(db, topic_id).await?.max(0),
    )
    .unwrap_or(0);
```

- [ ] **Step 5: Run `just ci`**

Run: `just ci` Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/reference_import/crawl.rs src/reference_import/file.rs src/reference_import/git.rs src/reference_import/update.rs
git commit -m "fix: safe i64→usize cast for reference counts

count_references_by_topic returns i64 from SQLite. A negative value
(impossible but not type-guaranteed) would wrap to a huge usize."
```

---

## Task 4: Fix u64→u32 Truncation in Browser Tool + Accessibility

**Files:**

- Modify: `src/tools/browser.rs:455-459,460-464,614-618,635-639`
- Modify: `src/web/browser/accessibility.rs:151`

- [ ] **Step 1: Fix `execute_resize` width/height (browser.rs:455-464)**

Replace:

```rust
    let width = params
        .get("width")
        .and_then(Value::as_u64)
        .ok_or_else(|| ToolError::InvalidParams("'resize' requires 'width' parameter".into()))?
        as u32;
    let height = params
        .get("height")
        .and_then(Value::as_u64)
        .ok_or_else(|| ToolError::InvalidParams("'resize' requires 'height' parameter".into()))?
        as u32;
```

With:

```rust
    let width = params
        .get("width")
        .and_then(Value::as_u64)
        .ok_or_else(|| ToolError::InvalidParams("'resize' requires 'width' parameter".into()))
        .and_then(|v| {
            u32::try_from(v)
                .map_err(|_| ToolError::InvalidParams(format!("width {v} exceeds u32 range")))
        })?;
    let height = params
        .get("height")
        .and_then(Value::as_u64)
        .ok_or_else(|| ToolError::InvalidParams("'resize' requires 'height' parameter".into()))
        .and_then(|v| {
            u32::try_from(v)
                .map_err(|_| ToolError::InvalidParams(format!("height {v} exceeds u32 range")))
        })?;
```

- [ ] **Step 2: Fix `execute_focus` tab_id (browser.rs:614-618)**

Replace the `as u32` cast:

```rust
    let tab_id = params
        .get("tab")
        .and_then(Value::as_u64)
        .ok_or_else(|| ToolError::InvalidParams("'focus' requires 'tab' parameter".into()))
        .and_then(|v| {
            u32::try_from(v)
                .map_err(|_| ToolError::InvalidParams(format!("tab id {v} exceeds u32 range")))
        })?;
```

- [ ] **Step 3: Fix `execute_close` tab_id (browser.rs:635-639)**

Same pattern as Step 2:

```rust
    let tab_id = params
        .get("tab")
        .and_then(Value::as_u64)
        .ok_or_else(|| ToolError::InvalidParams("'close' requires 'tab' parameter".into()))
        .and_then(|v| {
            u32::try_from(v)
                .map_err(|_| ToolError::InvalidParams(format!("tab id {v} exceeds u32 range")))
        })?;
```

- [ ] **Step 4: Fix accessibility heading level (accessibility.rs:151)**

Replace:

```rust
                props.level = value.and_then(|v| v.as_u64()).map(|n| n as u32);
```

With:

```rust
                props.level = value
                    .and_then(|v| v.as_u64())
                    .and_then(|n| u32::try_from(n).ok());
```

- [ ] **Step 5: Run `just ci`**

Run: `just ci` Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/tools/browser.rs src/web/browser/accessibility.rs
git commit -m "fix: safe u64→u32 casts in browser tool and accessibility

JSON values are parsed as u64 but browser APIs take u32. Values
above u32::MAX would silently truncate to a wrong tab/dimension."
```

---

## Task 5: Fix Unchecked Duration Subtraction

**Files:**

- Modify: `src/web/search.rs:19`

- [ ] **Step 1: Fix the Instant subtraction**

Replace:

```rust
    BRAVE_LAST_REQUEST.get_or_init(|| Mutex::new(Instant::now() - Duration::from_secs(60)))
```

With:

```rust
    BRAVE_LAST_REQUEST.get_or_init(|| {
        Mutex::new(
            Instant::now()
                .checked_sub(Duration::from_secs(60))
                .unwrap_or_else(Instant::now),
        )
    })
```

- [ ] **Step 2: Run `just ci`**

Run: `just ci` Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add src/web/search.rs
git commit -m "fix: use checked_sub for Instant arithmetic

Instant::now() - Duration panics if the system uptime is less than
the duration (possible on Windows shortly after boot)."
```

---

## Task 6: Fix Wildcard Match Hiding Enum Variants

**Files:**

- Modify: `src/reference_import/update.rs:190`

- [ ] **Step 1: Replace wildcard with explicit variant**

The `ImportSource` enum has three variants: `Git`, `Crawl`, `File`. The `fetch_manifest`
function handles `Git` and `Crawl`. The `_` arm catches `File`.

Replace:

```rust
        _ => Err(ImportError::Config(
            "only git and crawl sources support update".into(),
        )),
```

With:

```rust
        ImportSource::File { .. } => Err(ImportError::Config(
            "only git and crawl sources support update".into(),
        )),
```

- [ ] **Step 2: Run `just ci`**

Run: `just ci` Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add src/reference_import/update.rs
git commit -m "fix: match ImportSource::File explicitly instead of wildcard

The wildcard would silently catch any new ImportSource variants,
hiding compile-time exhaustiveness checks."
```

---

## Task 7: Remove Unused `async` from 8 Functions

These functions are `async` but contain no `.await` expressions. Removing `async` avoids
an unnecessary `Future` wrapper allocation.

**Files:**

- Modify: `src/cli/agent.rs:16`
- Modify: `src/cli/reset.rs:22`
- Modify: `src/cli/services.rs:31`
- Modify: `src/cli/skills.rs:24`
- Modify: `src/cli/status.rs:48`
- Modify: `src/tools/browser.rs:649` (`execute_tabs`)
- Modify: `src/web/browser/manager.rs:452,523`
- Modify: `src/main.rs` or callers — remove `.await` on call sites

- [ ] **Step 1: Remove `async` from `cli/agent.rs:16`**

Change `pub async fn execute(` to `pub fn execute(`. Then grep for callers and remove
`.await`:

```bash
rg "cli::agent::execute\b|agent::execute\(" src/
```

Update each caller to remove the `.await`.

- [ ] **Step 2: Remove `async` from `cli/reset.rs:22`**

Same pattern: `pub async fn execute(` → `pub fn execute(`. Update callers.

- [ ] **Step 3: Remove `async` from `cli/services.rs:31`**

Same pattern.

- [ ] **Step 4: Remove `async` from `cli/skills.rs:24`**

Same pattern.

- [ ] **Step 5: Remove `async` from `cli/status.rs:48`**

Same pattern.

- [ ] **Step 6: Remove `async` from `tools/browser.rs:649` (`execute_tabs`)**

This is a private function. Change `async fn execute_tabs(` → `fn execute_tabs(`. Update
its one call site in the same file — remove `.await`.

- [ ] **Step 7: Remove `async` from `web/browser/manager.rs:452` and `:523`**

Two methods on `BrowserManager`. Change both from `pub async fn` → `pub fn`. Grep for
callers:

```bash
rg "\.list_browsers\(|\.disconnect_browser\(" src/
```

Remove `.await` from each call site.

- [ ] **Step 8: Run `just ci`**

Run: `just ci` Expected: PASS. If any caller was missed, the compiler will flag
"expected `X`, found `impl Future<Output = X>`".

- [ ] **Step 9: Commit**

```bash
git add src/cli/agent.rs src/cli/reset.rs src/cli/services.rs src/cli/skills.rs src/cli/status.rs src/tools/browser.rs src/web/browser/manager.rs src/main.rs
git commit -m "refactor: remove unused async from 8 functions

These functions contained no .await expressions. Removing async
avoids an unnecessary Future wrapper allocation at each call."
```

---

## Task 8: Introduce `NoteRecord` Struct for Knowledge DB Functions

This is the biggest refactor — one struct eliminates `too_many_arguments` from 6
functions across 3 layers (DB, tool, CLI) and all their call sites (~40 sites in
production code + tests).

**Files:**

- Modify: `src/db/knowledge/crud.rs:1-110` — define `NoteRecord`, refactor
  `create_note_full` + `update_note`
- Modify: `src/tools/note_write.rs:304-617` — update `create_note` + `update_note`
- Modify: `src/cli/note.rs:74-237` — update `create_note` + `update_note`
- Modify: `src/cli/knowledge.rs` — update call sites
- Modify: `src/daemon/watcher.rs` — update call sites
- Modify: `src/knowledge/reconcile.rs` — update call site
- Modify: `tests/knowledge.rs` — update all `create_note_full` + `update_note` calls
- Modify: `tests/embeddings.rs` — update all `create_note_full` calls
- Modify: `tests/embedding_live.rs` — update `create_note_full` call

- [ ] **Step 1: Define `NoteRecord` struct in `src/db/knowledge/crud.rs`**

Add at the top of the file, after imports:

```rust
/// Parameters for creating or updating a knowledge note in the database.
#[derive(Debug, Default)]
pub struct NoteRecord<'a> {
    pub title: &'a str,
    pub body: &'a str,
    pub tags: &'a [String],
    pub sources: &'a [String],
    pub trust: i64,
    pub archetype: Option<&'a str>,
    pub topic_id: Option<&'a str>,
    pub path: Option<&'a str>,
    pub file_hash: Option<&'a str>,
}
```

- [ ] **Step 2: Refactor `create_note_full` to use `NoteRecord`**

Change the signature from 10 params to 2:

```rust
#[tracing::instrument(skip_all, level = "debug", fields(title = %note.title))]
pub async fn create_note_full(
    db: &SqlitePool,
    note: &NoteRecord<'_>,
) -> Result<String, DatabaseError> {
```

Update the function body to use `note.title`, `note.body`, `note.tags`, etc. instead of
the individual params. The SQL and binds stay the same — just prefix with `note.`.

- [ ] **Step 3: Update the simple `create_note` wrapper**

```rust
pub async fn create_note(
    db: &SqlitePool,
    title: &str,
    body: &str,
) -> Result<String, DatabaseError> {
    create_note_full(
        db,
        &NoteRecord {
            title,
            body,
            trust: 5,
            ..Default::default()
        },
    )
    .await
}
```

- [ ] **Step 4: Refactor `update_note` to use `NoteRecord`**

Change the signature from 10 params to 3:

```rust
#[tracing::instrument(skip_all, level = "debug", fields(note_id = %note_id))]
pub async fn update_note(
    db: &SqlitePool,
    note_id: &str,
    note: &NoteRecord<'_>,
) -> Result<(), DatabaseError> {
```

Update the function body accordingly.

- [ ] **Step 5: Remove `#[allow(clippy::too_many_arguments)]` from both functions**

Delete the `#[allow(clippy::too_many_arguments)]` lines above `create_note_full`
(line 21) and `update_note` (line 68).

- [ ] **Step 6: Update `src/tools/note_write.rs` — `create_note`**

Change the method signature:

```rust
    async fn create_note(
        &self,
        ctx: &ToolContext,
        note: &NoteRecord<'_>,
        parent: Option<&str>,
    ) -> Result<String, ToolError> {
```

Update the body: replace all individual field accesses with `note.field`. The
`db::knowledge::create_note_full` call becomes
`db::knowledge::create_note_full(&ctx.db, note)`. Remove the
`#[allow(clippy::too_many_arguments)]` attribute.

Update the call site within `note_write.rs` that calls this method — it currently passes
individual fields parsed from the tool params. Build a `NoteRecord` from those fields
and pass it.

- [ ] **Step 7: Update `src/tools/note_write.rs` — `update_note`**

Same pattern as Step 6. Change signature, update body, update call site.

- [ ] **Step 8: Update `src/cli/note.rs` — both functions**

Change both `create_note` and `update_note` to take `NoteRecord` instead of individual
params:

```rust
async fn create_note(
    db: &GhostDb,
    workspace: &std::path::Path,
    note: &NoteRecord<'_>,
) -> Result<String, GhostError> {
```

The CLI layer doesn't pass `archetype`, `topic_id`, `path`, `file_hash` — those will be
`None` / default via
`NoteRecord { title, body, tags, sources, trust, ..Default::default() }` at the call
site. Remove `#[allow(clippy::too_many_arguments)]` from both.

- [ ] **Step 9: Update remaining production call sites**

Grep for all remaining callers and update them:

- `src/cli/knowledge.rs` — two call sites (one for create, one for update). Build
  `NoteRecord` from the parsed TOML/frontmatter values.
- `src/daemon/watcher.rs` — two call sites (one create, one update). Build `NoteRecord`
  from the file watcher fields.
- `src/knowledge/reconcile.rs` — one create call site. Build `NoteRecord` from
  reconciliation data.

- [ ] **Step 10: Update test call sites**

Many call sites in `tests/knowledge.rs`, `tests/embeddings.rs`,
`tests/embedding_live.rs`. Each currently passes 10 positional args to
`create_note_full`. Replace with `NoteRecord { ... }`.

Example — the most common test pattern:

```rust
// Before:
db::knowledge::create_note_full(&db, "Title", "body", &[], &[], 5, None, None, None, None)

// After:
db::knowledge::create_note_full(&db, &NoteRecord { title: "Title", body: "body", trust: 5, ..Default::default() })
```

For calls that set more fields:

```rust
// Before:
db::knowledge::create_note_full(&db, "Title", "body", &tags, &sources, 8, Some("entity"), Some("topic"), Some("/path"), Some("hash"))

// After:
db::knowledge::create_note_full(&db, &NoteRecord {
    title: "Title",
    body: "body",
    tags: &tags,
    sources: &sources,
    trust: 8,
    archetype: Some("entity"),
    topic_id: Some("topic"),
    path: Some("/path"),
    file_hash: Some("hash"),
})
```

- [ ] **Step 11: Run `just ci`**

Run: `just ci` Expected: PASS.

- [ ] **Step 12: Commit**

```bash
git add src/db/knowledge/crud.rs src/tools/note_write.rs src/cli/note.rs src/cli/knowledge.rs src/daemon/watcher.rs src/knowledge/reconcile.rs tests/knowledge.rs tests/embeddings.rs tests/embedding_live.rs
git commit -m "refactor: introduce NoteRecord struct for knowledge DB functions

Replaces 8-10 positional parameters with a single NoteRecord<'a>
struct across create_note_full, update_note, and their callers in
the tool, CLI, watcher, and reconciliation layers. Removes all
too_many_arguments allows from note-related functions."
```

---

## Task 9: Introduce `MessagePayload` Struct for Session DB Functions

**Files:**

- Modify: `src/db/sessions.rs:280-360` — define `MessagePayload`, refactor both
  functions
- Modify: `src/chat/session.rs` — update all call sites (~10 sites)
- Modify: `tests/common.rs` — update call site
- Modify: `tests/chat_orchestration.rs` — update call sites (~5 sites)
- Modify: `tests/database.rs` — update call sites (~2 sites)
- Modify: `tests/providers/out_of_sync_live.rs` — update call site
- Modify: `tests/providers/message_adjacency_live.rs` — update call sites (~2 sites)

- [ ] **Step 1: Define `MessagePayload` in `src/db/sessions.rs`**

Add near the top of the file:

```rust
/// Optional metadata fields for a chat message.
#[derive(Debug, Default)]
pub struct MessagePayload {
    pub tool_calls: Option<Vec<serde_json::Value>>,
    pub tool_results: Option<Vec<serde_json::Value>>,
    pub raw_output: Option<Vec<serde_json::Value>>,
    pub images: Option<Vec<serde_json::Value>>,
}
```

- [ ] **Step 2: Refactor `create_message_with_metadata`**

Change the signature:

```rust
#[tracing::instrument(skip_all, level = "debug", fields(session_id = %session_id, role = %role))]
pub async fn create_message_with_metadata(
    db: &SqlitePool,
    session_id: &str,
    role: &str,
    content: &str,
    payload: &MessagePayload,
) -> Result<String, DatabaseError> {
    create_message_with_timestamp(
        db,
        session_id,
        role,
        content,
        payload,
        &chrono::Utc::now().to_rfc3339(),
    )
    .await
}
```

Remove the `#[allow(clippy::too_many_arguments)]`.

- [ ] **Step 3: Refactor `create_message_with_timestamp`**

```rust
#[tracing::instrument(skip_all, level = "debug", fields(session_id = %session_id, role = %role))]
pub async fn create_message_with_timestamp(
    db: &SqlitePool,
    session_id: &str,
    role: &str,
    content: &str,
    payload: &MessagePayload,
    created_at: &str,
) -> Result<String, DatabaseError> {
```

Update the body to use `payload.tool_calls`, `payload.tool_results`, etc. Remove
`#[allow(clippy::too_many_arguments)]`.

- [ ] **Step 4: Update the simple `create_message` wrapper**

```rust
pub async fn create_message(
    db: &SqlitePool,
    session_id: &str,
    role: &str,
    content: &str,
) -> Result<String, DatabaseError> {
    create_message_with_metadata(db, session_id, role, content, &MessagePayload::default()).await
}
```

- [ ] **Step 5: Update production call sites in `src/chat/session.rs`**

There are ~10 call sites. Each currently passes individual `Option<Vec<Value>>` params.
Replace with `MessagePayload { field: value, ..Default::default() }`.

Example — the most common pattern with tool calls:

```rust
// Before:
db::sessions::create_message_with_metadata(
    self.session_chat.db(),
    &self.session_thing,
    "assistant",
    &message,
    Some(tool_calls),
    None,
    Some(raw_output),
    None,
)

// After:
db::sessions::create_message_with_metadata(
    self.session_chat.db(),
    &self.session_thing,
    "assistant",
    &message,
    &MessagePayload {
        tool_calls: Some(tool_calls),
        raw_output: Some(raw_output),
        ..Default::default()
    },
)
```

- [ ] **Step 6: Update test call sites**

Update callers in `tests/common.rs`, `tests/chat_orchestration.rs`, `tests/database.rs`,
`tests/providers/out_of_sync_live.rs`, `tests/providers/message_adjacency_live.rs`.

- [ ] **Step 7: Run `just ci`**

Run: `just ci` Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add src/db/sessions.rs src/chat/session.rs tests/common.rs tests/chat_orchestration.rs tests/database.rs tests/providers/
git commit -m "refactor: introduce MessagePayload struct for session DB functions

Replaces 4 Option<Vec<Value>> parameters with a single MessagePayload
struct in create_message_with_metadata and create_message_with_timestamp.
Removes too_many_arguments allows."
```

---

## Task 10: Introduce `ToolLoopContext` for `run_tool_loop` and `run_agent_with_history`

**Files:**

- Modify: `src/chat/tool_loop.rs:112-124` — define `ToolLoopContext`, refactor
  `run_tool_loop`
- Modify: `src/chat/session.rs` — update 5 call sites + refactor
  `run_agent_with_history`

- [ ] **Step 1: Define `ToolLoopContext` in `src/chat/tool_loop.rs`**

Add before `run_tool_loop`:

```rust
/// Contextual channels and identifiers threaded through the tool loop.
pub(super) struct ToolLoopContext {
    pub event_tx: Option<EventSender>,
    pub interrupt_rx: Option<InterruptReceiver>,
    pub channel_id: Option<String>,
}
```

Note: `event_tx` is `Option<EventSender>` (owned) — the function will take
`&ToolLoopContext` and pass `ctx.event_tx.as_ref()` where needed. Check how `event_tx`
is used in the function body — if it's passed as `Option<&EventSender>`, the struct
field should match. Read the current usages carefully before finalizing. If `event_tx`
is always used by reference, keep `Option<&'a EventSender>` with a lifetime:

```rust
pub(super) struct ToolLoopContext<'a> {
    pub event_tx: Option<&'a EventSender>,
    pub interrupt_rx: Option<InterruptReceiver>,
    pub channel_id: Option<String>,
}
```

- [ ] **Step 2: Refactor `run_tool_loop` signature**

```rust
pub(super) async fn run_tool_loop(
    session_chat: &SessionChat,
    session_id: &str,
    model: &str,
    max_iterations: usize,
    reasoning_effort: ReasoningEffort,
    handler: &mut (impl ToolLoopHandler + ?Sized),
    history: &mut Vec<ChatMessage>,
    ctx: ToolLoopContext<'_>,
) -> Result<(ChatResult, RunMetadata), ChatError> {
```

Update the function body: replace `event_tx` with `ctx.event_tx`, `interrupt_rx` with
`ctx.interrupt_rx` (may need `mut ctx` since interrupt_rx is consumed), `channel_id`
with `ctx.channel_id`.

Remove `#[allow(clippy::too_many_arguments)]`.

- [ ] **Step 3: Update the 5 call sites in `src/chat/session.rs`**

Each call site currently passes `event_tx, Some(int_rx), channel_id` as the last 3 args.
Replace with:

```rust
        let result = run_tool_loop(
            self,
            session_id,
            &model,
            self.max_tool_iterations,
            effort,
            &mut handler,
            &mut history,
            ToolLoopContext {
                event_tx,
                interrupt_rx: Some(int_rx),
                channel_id,
            },
        )
        .await;
```

- [ ] **Step 4: Refactor `run_agent_with_history`**

Change the signature to take `ToolLoopContext` instead of separate `channel_id` +
`event_tx`:

```rust
    pub async fn run_agent_with_history(
        &self,
        session_id: &str,
        system_prompt: String,
        messages: &[crate::scripting::LuaMessage],
        db_message_count: usize,
        config: &crate::scripting::AgentConfig,
        script_host: &ScriptHost,
        ctx: ToolLoopContext<'_>,
    ) -> Result<(ChatResult, RunMetadata), ChatError> {
```

Update the body to pass `ctx` through to `run_tool_loop`. Remove
`#[allow(clippy::too_many_arguments)]`. Update callers (grep for
`run_agent_with_history`).

- [ ] **Step 5: Run `just ci`**

Run: `just ci` Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/chat/tool_loop.rs src/chat/session.rs
git commit -m "refactor: introduce ToolLoopContext for run_tool_loop

Bundles event_tx, interrupt_rx, and channel_id into a struct,
reducing run_tool_loop from 10 to 8 parameters and
run_agent_with_history from 9 to 8 (including &self)."
```

---

## Task 11: Introduce `RenderState` for Accessibility `render_node`

**Files:**

- Modify: `src/web/browser/accessibility.rs:318-434` — define `RenderState`, refactor
  `render_node`

- [ ] **Step 1: Define `RenderState` struct**

Add before `render_node`:

```rust
/// Mutable traversal state threaded through recursive render_node calls.
struct RenderState<'a> {
    refs: &'a mut RefMap,
    buf: &'a mut String,
    counter: &'a mut usize,
    rendered: &'a mut usize,
    truncated: &'a mut bool,
}
```

- [ ] **Step 2: Refactor `render_node` signature**

```rust
fn render_node(
    node: &AxNode,
    state: &mut RenderState<'_>,
    max_nodes: usize,
    max_depth: usize,
    offset: usize,
    depth: usize,
    total: usize,
)
```

That's 7 params — under the threshold. The immutable config params (`max_nodes`,
`max_depth`, `offset`, `total`) stay as direct args because they're Copy types passed
down unchanged. The mutable state is bundled.

- [ ] **Step 3: Update the function body**

Replace all occurrences of `refs` → `state.refs`, `buf` → `state.buf`, `counter` →
`state.counter`, `rendered` → `state.rendered`, `truncated` → `state.truncated`.

For the recursive call to `render_node`, pass `state` through:

```rust
render_node(child, state, max_nodes, max_depth, offset, depth + 1, total);
```

Remove `#[allow(clippy::too_many_arguments)]`.

- [ ] **Step 4: Update the caller of `render_node`**

Find the public function that calls `render_node` (likely `render_tree` or similar in
the same file). Build the `RenderState` struct and pass it:

```rust
let mut state = RenderState {
    refs: &mut refs,
    buf: &mut buf,
    counter: &mut counter,
    rendered: &mut rendered,
    truncated: &mut truncated,
};
render_node(root, &mut state, max_nodes, max_depth, offset, 0, total);
```

- [ ] **Step 5: Run `just ci`**

Run: `just ci` Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/web/browser/accessibility.rs
git commit -m "refactor: bundle mutable traversal state into RenderState

Reduces render_node from 11 to 7 parameters by grouping the 5
mutable state references (refs, buf, counter, rendered, truncated)
into a RenderState struct."
```

---

## Task 12: Remove Global `too_many_arguments` Allow

**Files:**

- Modify: `src/lib.rs:1-2`

- [ ] **Step 1: Remove the global allow**

Delete line 1 (`// TODO: Remove that and cleanup all complex functions!`) and line 2
(`#![allow(clippy::too_many_arguments)]`) from `src/lib.rs`.

- [ ] **Step 2: Run `just ci`**

Run: `just ci` Expected: PASS — all functions should now be under 7 params. If any
function was missed, the compiler will emit `clippy::too_many_arguments` and you need to
go back and refactor it.

- [ ] **Step 3: Commit**

```bash
git add src/lib.rs
git commit -m "chore: remove global allow(clippy::too_many_arguments)

All functions are now under the 7-parameter threshold after
introducing NoteRecord, MessagePayload, ToolLoopContext, and
RenderState structs."
```
