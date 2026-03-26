# Compaction Persistence and Write-Time Capping

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or superpowers:executing-plans
> to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stop phase 1 masking from permanently degrading LLM response quality by (A)
capping tool results at write time and (B) persisting masked content so future turns
don't re-mask everything.

**Architecture:** Two independent changes to the message storage and compaction
pipelines. Change A introduces a capping layer between tool execution and DB persistence
— oversized results are written to an overflow file and replaced with a head+tail
preview. Change B adds a `compacted` column to the messages table so phase 1 masking is
a one-time, persisted operation rather than a transient re-application every turn.

**Design spec:** `backlog/tasks/2026-03-26-compaction-persistence-and-capping.md`

**Tech Stack:** Rust, SQLite (sqlx migrations), existing compaction in
`src/chat/compaction.rs`, message storage in `src/db/sessions.rs`

---

## File Map

| File                           | Action | Responsibility                                                        |
| ------------------------------ | ------ | --------------------------------------------------------------------- |
| `src/chat/tool_cap.rs`         | Create | Tool result capping logic (head+tail, overflow file write)            |
| `src/chat/mod.rs`              | Modify | Add `mod tool_cap;`                                                   |
| `src/config_workspace.rs`      | Modify | Add `.tool-overflow/` to bootstrapped directories                     |
| `src/config.rs`                | Modify | Add `max_tool_result_chars` to `CompactionConfig`                     |
| `src/chat/session.rs`          | Modify | Wire capping into `on_tool_results` for all handlers                  |
| `migrations/014_compacted.sql` | Create | Add `compacted` column to message table                               |
| `src/db/sessions.rs`           | Modify | Add `update_message_compacted()`, add field to record                 |
| `src/chat/compaction.rs`       | Modify | Persist masking, skip already-compacted messages                      |
| `src/chat/session.rs`          | Modify | Update `load_provider_history` return type to include compacted flags |

---

### Task 1: Tool result capping module

Create the capping logic as a standalone pure function with no DB or IO dependencies
(overflow file writing is separate). This makes it easy to unit test.

**Files:**

- Create: `src/chat/tool_cap.rs`
- Modify: `src/chat/mod.rs`

- [ ] **Step 1: Write test for capping logic**

In `src/chat/tool_cap.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_content_passes_through() {
        let result = cap_tool_result("hello world", 100);
        assert!(result.is_none(), "should return None when under limit");
    }

    #[test]
    fn long_content_produces_head_tail_preview() {
        let content = "A".repeat(70) + &"B".repeat(30) + &"C".repeat(100);
        let result = cap_tool_result(&content, 100).unwrap();

        // Head is 70% of cap = 70 chars
        assert!(result.preview.starts_with(&"A".repeat(70)));
        // Tail is 30% of cap = 30 chars
        assert!(result.preview.ends_with(&"C".repeat(30)));
        // Contains marker
        assert!(result.preview.contains("full output saved to"));
        // Full content preserved
        assert_eq!(result.full_content, content);
    }

    #[test]
    fn preview_format_includes_placeholder_path() {
        let content = "X".repeat(200);
        let result = cap_tool_result(&content, 100).unwrap();

        // Placeholder uses {path} which the caller replaces
        assert!(result.preview.contains("{path}"));
    }

    #[test]
    fn head_tail_split_at_char_boundaries() {
        // Multi-byte UTF-8: each char is 4 bytes
        let content = "🎉".repeat(100);
        let result = cap_tool_result(&content, 40).unwrap();

        // Should not panic on char boundary issues
        assert!(result.preview.starts_with("🎉"));
        assert!(result.preview.ends_with("🎉"));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib chat::tool_cap::tests` Expected: FAIL — module does not exist.

- [ ] **Step 3: Implement capping logic**

In `src/chat/tool_cap.rs`:

```rust
/// Result of capping a tool result that exceeded the limit.
pub(super) struct CappedToolResult {
    /// Head+tail preview with a `{path}` placeholder for the overflow file path.
    pub preview: String,
    /// The original full content to write to the overflow file.
    pub full_content: String,
}

/// Check if a tool result exceeds the character limit. Returns `None` if the
/// content fits within `max_chars`, or a `CappedToolResult` with a head+tail
/// preview and the full content for overflow storage.
///
/// The preview contains a `{path}` placeholder that the caller must replace
/// with the actual overflow file path before storing.
pub(super) fn cap_tool_result(content: &str, max_chars: usize) -> Option<CappedToolResult> {
    if content.len() <= max_chars {
        return None;
    }

    let head_budget = max_chars * 7 / 10; // 70%
    let tail_budget = max_chars - head_budget; // 30%

    let head_end = safe_truncate(content, head_budget);
    let tail_start = safe_truncate_back(content, tail_budget);

    let preview = format!(
        "{}\n\n... [full output saved to {{path}}] ...\n\n{}",
        &content[..head_end],
        &content[tail_start..],
    );

    Some(CappedToolResult {
        preview,
        full_content: content.to_string(),
    })
}

/// Find the largest byte index <= `max_bytes` that is a valid char boundary.
fn safe_truncate(s: &str, max_bytes: usize) -> usize {
    if max_bytes >= s.len() {
        return s.len();
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    end
}

/// Find the smallest byte index such that `s[index..]` is at most `max_bytes`
/// and starts on a char boundary.
fn safe_truncate_back(s: &str, max_bytes: usize) -> usize {
    if max_bytes >= s.len() {
        return 0;
    }
    let mut start = s.len() - max_bytes;
    while start < s.len() && !s.is_char_boundary(start) {
        start += 1;
    }
    start
}
```

- [ ] **Step 4: Add module declaration**

In `src/chat/mod.rs`, add:

```rust
mod tool_cap;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib chat::tool_cap::tests` Expected: PASS

- [ ] **Step 6: Commit**

```
git add src/chat/tool_cap.rs src/chat/mod.rs
git commit -m "feat: add tool result capping logic (head+tail preview)"
```

---

### Task 2: Overflow file writing and wiring into tool result storage

Wire the capping into the `on_tool_results` path for all three handlers. Write overflow
files to `$WORKSPACE/.tool-overflow/`. Add `.tool-overflow/` to the workspace bootstrap.

**Files:**

- Modify: `src/config_workspace.rs`
- Modify: `src/config.rs`
- Modify: `src/chat/tool_cap.rs`
- Modify: `src/chat/session.rs`

- [ ] **Step 1: Add `.tool-overflow/` to workspace bootstrap**

In `src/config_workspace.rs`, add `".tool-overflow"` to the directory list in
`bootstrap_workspace_dirs()`:

```rust
    for dir in [
        "skills",
        "agents",
        ".cache",
        ".tool-overflow",
        "notes",
        // ... rest unchanged
    ] {
```

- [ ] **Step 2: Add `max_tool_result_chars` to config**

In `src/config.rs`, add to `CompactionSettings`:

```rust
pub struct CompactionSettings {
    pub threshold: Option<f64>,
    pub mask_preview_chars: Option<usize>,
    pub max_tool_result_chars: Option<usize>,
    pub instructions: Option<String>,
}
```

And to `CompactionConfig`:

```rust
pub struct CompactionConfig {
    pub threshold: f64,
    pub mask_preview_chars: usize,
    pub max_tool_result_chars: usize,
    pub instructions: Option<String>,
}
```

Default value: `30_000` (roughly 7.5K tokens). Set this where other compaction defaults
are applied in `Config::from_settings()` and in `test_config()`.

- [ ] **Step 3: Add overflow file writer to `tool_cap.rs`**

Add an async function that writes the overflow file and returns the finalized preview
with the real path substituted in:

```rust
use std::path::Path;

/// Write the full content to an overflow file and return the preview with the
/// path filled in. The file is named `{message_id}.txt` under the
/// `.tool-overflow/` workspace directory.
pub(super) async fn write_overflow_file(
    workspace: &Path,
    message_id: &str,
    capped: CappedToolResult,
) -> String {
    let overflow_dir = workspace.join(".tool-overflow");
    let filename = format!("{message_id}.txt");
    let file_path = overflow_dir.join(&filename);

    match tokio::fs::write(&file_path, &capped.full_content).await {
        Ok(()) => capped
            .preview
            .replace("{path}", &format!(".tool-overflow/{filename}")),
        Err(e) => {
            tracing::warn!(
                error = %e,
                path = %file_path.display(),
                "Failed to write tool overflow file — storing uncapped",
            );
            capped.full_content
        }
    }
}
```

- [ ] **Step 4: Add a `cap_content_blocks` function**

This function takes the `Vec<ContentBlock>` from tool execution and caps any oversized
`ToolResult` blocks. It needs workspace path and config to do its work. Add to
`tool_cap.rs`:

```rust
use crate::providers::ContentBlock;

/// Cap any oversized tool result text in the content blocks. Writes overflow
/// files for results that exceed the limit. Returns the (possibly modified)
/// blocks.
pub(super) async fn cap_content_blocks(
    blocks: Vec<ContentBlock>,
    workspace: &Path,
    max_chars: usize,
) -> Vec<ContentBlock> {
    let mut result = Vec::with_capacity(blocks.len());
    for block in blocks {
        match block {
            ContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                let content = match cap_tool_result(&content, max_chars) {
                    Some(capped) => {
                        write_overflow_file(workspace, &tool_use_id, capped).await
                    }
                    None => content,
                };
                result.push(ContentBlock::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                });
            }
            other => result.push(other),
        }
    }
    result
}
```

- [ ] **Step 5: Wire capping into the tool loop**

The cleanest insertion point is in `tool_loop.rs` right after
`session_chat.execute_tool_calls()` returns and before `handler.on_tool_results()` is
called. This way all three handlers benefit without duplicating the call.

In `src/chat/tool_loop.rs`, after the `execute_tool_calls` call and before the
`on_tool_results` call:

```rust
let tool_results = session_chat
    .execute_tool_calls(/* ... */)
    .await;

// Cap oversized tool results before storage
let config = session_chat.config();
let tool_results = tool_cap::cap_content_blocks(
    tool_results,
    &config.workspace,
    config.compaction.max_tool_result_chars,
).await;

handler.on_tool_results(&tool_results).await?;
```

This requires making `tool_cap::cap_content_blocks` visible from `tool_loop.rs` (both
are in the `chat` module, so `pub(super)` access works).

- [ ] **Step 6: Run full test suite**

Run: `cargo test` Expected: PASS. Existing tests use small tool results that won't
trigger capping.

- [ ] **Step 7: Commit**

```
git add src/config_workspace.rs src/config.rs src/chat/tool_cap.rs src/chat/tool_loop.rs
git commit -m "feat: cap oversized tool results at write time

Tool results exceeding max_tool_result_chars (default 30K) are truncated
to a head+tail preview with the full output saved to
.tool-overflow/{id}.txt in the workspace. Capping runs in the tool loop
before on_tool_results so all handlers benefit."
```

---

### Task 3: Add `compacted` column to messages table

Add the migration and update the DB layer.

**Files:**

- Create: `migrations/014_compacted.sql`
- Modify: `src/db/sessions.rs`

- [ ] **Step 1: Create migration**

In `migrations/014_compacted.sql`:

```sql
ALTER TABLE message ADD COLUMN compacted INTEGER NOT NULL DEFAULT 0;
```

- [ ] **Step 2: Add field to `MessageRecord`**

In `src/db/sessions.rs`, add to the `MessageRecord` struct:

```rust
pub struct MessageRecord {
    pub id: String,
    pub session_id: String,
    pub role: String,
    pub content: String,
    pub tool_calls: Option<String>,
    pub tool_results: Option<String>,
    pub raw_output: Option<String>,
    pub images: Option<String>,
    pub created_at: String,
    pub compacted: bool,
}
```

- [ ] **Step 3: Add `update_message_compacted` function**

In `src/db/sessions.rs`:

```rust
/// Mark a message as compacted, replacing its tool_calls and tool_results
/// with the masked versions.
#[tracing::instrument(skip_all, level = "debug", fields(message_id = %message_id))]
pub async fn update_message_compacted(
    db: &SqlitePool,
    message_id: &str,
    tool_calls: Option<&str>,
    tool_results: Option<&str>,
) -> Result<(), DatabaseError> {
    sqlx::query(
        "UPDATE message SET tool_calls = ?, tool_results = ?, compacted = 1 WHERE id = ?",
    )
    .bind(tool_calls)
    .bind(tool_results)
    .bind(message_id)
    .execute(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "message",
        operation: "update_message_compacted",
        source,
    })?;
    Ok(())
}
```

- [ ] **Step 4: Run tests to check migration applies**

Run: `cargo test` Expected: PASS. The migration should apply cleanly. The new
`compacted` field on `MessageRecord` is populated by sqlx's `FromRow` derive from the
new column. Existing messages get `compacted = 0` (false) from the default.

- [ ] **Step 5: Commit**

```
git add migrations/014_compacted.sql src/db/sessions.rs
git commit -m "feat: add compacted column to messages table

Tracks whether a message's tool content has been masked by phase 1
compaction. Masked messages are loaded as-is on future turns instead
of being re-masked."
```

---

### Task 4: Persist phase 1 masking and skip already-compacted messages

The core change: when `run_compaction` applies phase 1, persist the masked content for
each affected message and set `compacted = true`. On future turns, those messages load
already-masked and don't contribute to re-triggering compaction.

**Files:**

- Modify: `src/chat/compaction.rs`
- Modify: `src/chat/session.rs`

- [ ] **Step 1: Write test for skipping already-compacted messages**

Add to the test module in `src/chat/compaction.rs`. The test verifies that
`mask_tool_interactions` does not re-mask messages that are already compacted (simulated
by checking that content already matching the masked format passes through unchanged).

This test validates the behavioral change: if a message was already masked in a previous
compaction run, it should not be masked again.

```rust
#[test]
fn mask_skips_already_compacted_messages() {
    // Simulate a message that was previously compacted: its tool result
    // already looks like a masked placeholder
    let already_masked = "[tool_result: web_fetch — https://example.com... (truncated)]";
    let messages = vec![
        user_text("Hello"),
        assistant_with_tool("Searching", "tu_1", "web_fetch"),
        // This tool result is already masked from a previous compaction
        ChatMessage {
            role: Role::User,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "tu_1".to_string(),
                content: already_masked.to_string(),
                is_error: false,
            }],
        },
        user_text("Thanks"),
    ];

    let compacted_ids = vec![false, false, true, false];
    let masked = mask_tool_interactions_with_compacted(
        &messages, 3, 100, &compacted_ids,
    );

    // The already-compacted message should pass through unchanged
    if let ContentBlock::ToolResult { content, .. } = &masked[2].content[0] {
        assert_eq!(content, already_masked);
    } else {
        panic!("expected tool result");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib chat::compaction::tests::mask_skips_already_compacted_messages`
Expected: FAIL — `mask_tool_interactions_with_compacted` does not exist.

- [ ] **Step 3: Add `compacted` flag to masking function**

Extend `mask_tool_interactions` to accept an optional slice of `compacted` booleans (one
per message). Messages where `compacted[i]` is true are cloned as-is regardless of their
position relative to `keep_start`.

The cleanest approach: add a new function `mask_tool_interactions_with_compacted` that
takes the extra parameter, and have the existing `mask_tool_interactions` call it with
an all-false slice for backward compatibility:

```rust
pub fn mask_tool_interactions(
    messages: &[ChatMessage],
    keep_start: usize,
    preview_chars: usize,
) -> Vec<ChatMessage> {
    let no_compacted = vec![false; messages.len()];
    mask_tool_interactions_with_compacted(messages, keep_start, preview_chars, &no_compacted)
}

pub fn mask_tool_interactions_with_compacted(
    messages: &[ChatMessage],
    keep_start: usize,
    preview_chars: usize,
    compacted: &[bool],
) -> Vec<ChatMessage> {
    messages
        .iter()
        .enumerate()
        .map(|(i, msg)| {
            // Already compacted or in the current turn — pass through
            if i >= keep_start || compacted.get(i).copied().unwrap_or(false) {
                return msg.clone();
            }
            // ... existing masking logic for ToolUse, ToolResult, Image blocks
        })
        .collect()
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib chat::compaction::tests::mask_skips_already_compacted_messages`
Expected: PASS

- [ ] **Step 5: Thread `compacted` flags through `run_compaction`**

In `run_compaction`, the `stored_message_ids` parallel array already matches the
history. We need a parallel `compacted` array too. This comes from the
`MessageRecord.compacted` field loaded during `load_provider_history`.

Update `load_provider_history` to also return the `compacted` flags alongside the
messages and IDs. Change the return type from `(Vec<ChatMessage>, Vec<String>)` to
`(Vec<ChatMessage>, Vec<String>, Vec<bool>)`.

The compaction summary pseudo-message (if present) gets `compacted: false` since it's a
system message, not a tool result.

Thread this through:

- `compact_if_needed` — already receives `stored_message_ids`, add `compacted_flags`
- `compact_in_tool_loop_with_config` — builds the parallel arrays from DB, add compacted
- `run_compaction` — pass to `mask_tool_interactions_with_compacted`

- [ ] **Step 6: Persist masking results to DB**

In `run_compaction`, after `mask_tool_interactions_with_compacted` produces the masked
messages, identify which messages were newly masked (not in the current turn, not
already compacted) and persist them:

```rust
// After masking, persist newly-masked messages
for (i, (masked_msg, original_msg)) in masked.iter().zip(history.iter()).enumerate() {
    if i >= keep_start || compacted_flags.get(i).copied().unwrap_or(false) {
        continue; // Current turn or already persisted
    }

    // Check if this message has tool content that was masked
    let has_tool_content = original_msg.content.iter().any(|b| {
        matches!(b, ContentBlock::ToolUse { .. } | ContentBlock::ToolResult { .. })
    });
    if !has_tool_content {
        continue;
    }

    let msg_id = &stored_message_ids[i];
    if msg_id.is_empty() {
        continue; // Summary pseudo-message
    }

    // Serialize the masked tool_calls and tool_results
    let masked_tool_calls = tool_calls_to_json(&masked_msg.content);
    let masked_tool_results = tool_results_to_json(&masked_msg.content);

    if let Err(e) = db::sessions::update_message_compacted(
        self.db(),
        msg_id,
        masked_tool_calls.as_deref(),
        masked_tool_results.as_deref(),
    )
    .await
    {
        tracing::warn!(
            error = %e,
            message_id = msg_id,
            "Failed to persist compacted message",
        );
    }
}
```

Add small helper functions `tool_calls_to_json` and `tool_results_to_json` in
`compaction.rs` that extract and serialize the relevant `ContentBlock` variants from a
message's content — reusing the same JSON shape that `convert.rs` uses
(`{id, name, input}` for tool calls, `{tool_use_id, content, is_error}` for results).

- [ ] **Step 7: Run full test suite**

Run: `cargo test` Expected: PASS

- [ ] **Step 8: Commit**

```
git add src/chat/compaction.rs src/chat/session.rs
git commit -m "feat: persist phase 1 masking to DB

Phase 1 compaction now writes masked tool content back to the message
row and sets compacted=true. On future loads, already-compacted
messages are skipped by the masking function. This means compaction
is a one-time operation — subsequent turns that stay under threshold
see full tool results for recent messages."
```

---

### Task 5: Final integration test and CI

Run the full CI pipeline and verify everything works together.

**Files:**

- No new files

- [ ] **Step 1: Run `just ci`**

Run: `just ci` Expected: format + check + clippy + tests all pass.

- [ ] **Step 2: Verify migration applies on fresh DB**

Run: `cargo test -- --ignored` (if there are ignored integration tests) or run a quick
smoke test:

```
cargo run -- daemon --help
```

This exercises the migration path without needing a full daemon boot.

- [ ] **Step 3: Commit any fixups**

If `just ci` reveals formatting or clippy issues, fix and commit:

```
git add -u
git commit -m "fix: address clippy and formatting issues"
```
