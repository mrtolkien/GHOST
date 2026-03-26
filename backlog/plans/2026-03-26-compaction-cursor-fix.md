# Compaction Cursor Fix — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or superpowers:executing-plans
> to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the bug where Phase 2 compaction during a tool loop persists an empty
cursor, causing all messages to be dropped on reload — the model then only sees the
summary and answers old topics.

**Architecture:** Unify the two compaction paths (`compact_if_needed` and
`compact_in_tool_loop`) into a single `run_compaction` method that takes
`stored_message_ids` as a parameter. The tool loop path builds its IDs from the
in-memory history it already tracks. Add guards so an empty cursor can never be
persisted or treated as valid.

**Tech Stack:** Rust, SQLite (sqlx), existing compaction/session modules

---

## Root Cause

`compact_in_tool_loop_with_config` rebuilds `parallel_ids` from DB via
`get_session_message_ids`, but the in-memory `history` may have been modified by
`relocate_system_messages_between_tool_pairs` (which can merge/move messages) or
`repair_orphaned_tool_calls` (which can insert messages). When
`parallel_ids.len() != history.len()`, `summarize_older_messages` computes a `split`
index that is out of bounds for `parallel_ids`, falls through to `unwrap_or_default()` →
`""`, and persists an empty cursor. On reload, `load_provider_history` treats `Some("")`
as a valid cursor that matches no message, skipping the entire history.

## File Map

| File                                  | Change                                                                                                                                               |
| ------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- |
| `src/chat/compaction.rs`              | Extract `run_compaction`, delete `compact_in_tool_loop_with_config` body, add empty-cursor guard in `summarize_older_messages`                       |
| `src/chat/session.rs`                 | Update `compact_in_tool_loop` callers to pass IDs, add `Some("")` guard in `load_provider_history`, update `compact_if_needed` to call shared method |
| `src/chat/session.rs` (tests)         | Existing relocate tests untouched                                                                                                                    |
| `src/chat/compaction.rs` (tests)      | Add unit test for empty-cursor rejection                                                                                                             |
| `tests/providers/out_of_sync_live.rs` | Already exists — update to be a proper regression test                                                                                               |

---

### Task 1: Guard against empty cursor in `load_provider_history`

Defense-in-depth: even if a bad cursor slips through, never drop all messages.

**Files:**

- Modify: `src/chat/session.rs:425-426`

- [ ] **Step 1: Apply the guard**

The integration test for this guard is in Task 4. This step is the one-line fix.

In `src/chat/session.rs`, line 425:

```rust
let cursor = session.compaction_cursor_id.filter(|c| !c.is_empty());
```

- [ ] **Step 2: Run `just ci`**

Run: `just ci` Expected: All existing tests pass.

- [ ] **Step 3: Commit**

```bash
git add src/chat/session.rs
git commit -m "fix: treat empty compaction cursor as None to prevent dropping all messages"
```

---

### Task 2: Guard against empty cursor in `summarize_older_messages`

Prevent the root cause: never return an empty cursor from Phase 2. The cursor lookup in
`summarize_older_messages` uses `unwrap_or_default()` which silently produces `""` when
the index is out of bounds. Replace with an explicit error.

Note: `summarize_older_messages` uses `split` for both slicing messages AND looking up
the cursor ID, so we inline the guard rather than extracting a separate function.

**Files:**

- Modify: `src/chat/compaction.rs:275-282` (add error variant)
- Modify: `src/chat/compaction.rs:439-442` (replace `unwrap_or_default`)
- Test: `src/chat/compaction.rs` (unit tests)

- [ ] **Step 1: Add `CursorMismatch` error variant**

In `CompactionError` enum (`src/chat/compaction.rs:276`):

```rust
#[error("cursor mismatch: split={split} but stored_message_ids has {ids_len} entries")]
CursorMismatch { split: usize, ids_len: usize },
```

- [ ] **Step 2: Replace the silent fallback in `summarize_older_messages`**

Replace lines 439-442 in `src/chat/compaction.rs`:

```rust
// OLD:
let cursor_id = stored_message_ids
    .get(split.saturating_sub(1))
    .cloned()
    .unwrap_or_default();

// NEW:
let cursor_idx = split.saturating_sub(1);
let cursor_id = stored_message_ids
    .get(cursor_idx)
    .filter(|id| !id.is_empty())
    .cloned()
    .ok_or(CompactionError::CursorMismatch {
        split,
        ids_len: stored_message_ids.len(),
    })?;
```

- [ ] **Step 3: Write unit tests**

In `src/chat/compaction.rs` `mod tests`. These test the error propagation behavior of
`summarize_older_messages` indirectly by testing cursor computation logic. Since
`summarize_older_messages` is async and needs a provider, test the guard via the
`run_compaction` → `CursorMismatch` error path in Task 3's integration. For now, verify
the existing unit tests still pass.

- [ ] **Step 4: Run tests**

Run: `just ci` Expected: Existing tests pass. No unit test calls
`summarize_older_messages` directly (it's async + needs a provider), so the new error
variant is tested via Task 3/4.

- [ ] **Step 5: Commit**

```bash
git add src/chat/compaction.rs
git commit -m "fix: reject empty cursor ID in Phase 2 compaction instead of silent fallback"
```

---

### Task 3: Unify compaction into a single `run_compaction` method

Both `compact_if_needed` and `compact_in_tool_loop_with_config` are ~90% identical.
Extract the shared logic.

**Files:**

- Modify: `src/chat/compaction.rs:464-752`

- [ ] **Step 1: Extract `run_compaction` method on `SessionChat`**

```rust
/// Shared compaction logic for both pre-request and tool-loop paths.
///
/// `stored_message_ids` must be parallel to `history` — one DB message ID
/// per provider message. Callers are responsible for providing correct IDs.
///
/// Returns `true` when Phase 2 ran successfully.
async fn run_compaction(
    &self,
    session_id: &str,
    history: &mut Vec<ChatMessage>,
    stored_message_ids: &[String],
    compaction: &CompactionConfig,
) -> bool {
    let context_window = self.model_context_window();
    let tools = self.tool_manager().all_tool_schemas();

    let budget = compute_budget(context_window, "", &tools, history, compaction.threshold);

    if !budget.needs_compaction {
        return false;
    }

    tracing::info!(
        total = budget.total_estimated as u64,
        window = budget.context_window as u64,
        history = budget.history_tokens as u64,
        "Compaction triggered",
    );

    // Phase 1: mask tool interactions
    let keep_start = find_current_turn_start(history);
    let masked = mask_tool_interactions(history, keep_start, compaction.mask_preview_chars);
    let masked_tokens = estimate_history_tokens(&masked);

    tracing::debug!(
        before = budget.history_tokens as u64,
        after = masked_tokens as u64,
        saved = budget.history_tokens.saturating_sub(masked_tokens) as u64,
        "Phase 1: observation masking complete",
    );

    let total_after_mask = budget.system_tokens + budget.tool_tokens + masked_tokens;
    let still_over =
        total_after_mask as f64 > (budget.context_window as f64 * compaction.threshold);

    if !still_over {
        *history = masked;
        return false;
    }

    // Phase 2: LLM summarization
    tracing::info!("Masking insufficient — proceeding to Phase 2");

    let model_name = match self.default_model_name() {
        Ok(m) => m,
        Err(_) => {
            *history = masked;
            return false;
        }
    };

    let cache_key = session_id.to_string();
    match summarize_older_messages(
        self.provider(),
        &model_name,
        &cache_key,
        &masked,
        stored_message_ids,
        compaction,
        compaction.instructions.as_deref(),
    )
    .await
    {
        Ok(result) => {
            if let Err(e) = db::sessions::update_compaction(
                self.db(),
                session_id,
                &result.summary,
                &result.cursor_message_id,
            )
            .await
            {
                tracing::error!(
                    error = e.to_string(),
                    "Failed to persist compaction summary",
                );
                *history = masked;
                return false;
            }

            match self.load_provider_history(session_id).await {
                Ok((reloaded, _ids)) => *history = reloaded,
                Err(e) => {
                    tracing::error!(
                        error = e.to_string(),
                        "Failed to reload history after compaction",
                    );
                    *history = masked;
                }
            }
            true
        }
        Err(e) => {
            tracing::warn!(
                error = e.to_string(),
                "Phase 2 summarization failed — using masked history",
            );
            *history = masked;
            false
        }
    }
}
```

- [ ] **Step 2: Rewrite `compact_if_needed` to delegate**

```rust
pub(super) async fn compact_if_needed(
    &self,
    session_id: &str,
    history: &mut Vec<ChatMessage>,
    stored_message_ids: &[String],
) {
    let compaction = self.compaction_config();
    self.run_compaction(session_id, history, stored_message_ids, &compaction)
        .await;
}
```

- [ ] **Step 3: Rewrite `compact_in_tool_loop_with_config` to delegate**

The tool loop path must provide correct IDs. Since messages are persisted to DB before
`post_tool_iteration` is called, we can load them from DB — BUT we must verify the count
matches. If it doesn't, fall back to masked history (safe degradation).

```rust
pub(super) async fn compact_in_tool_loop_with_config(
    &self,
    session_id: &str,
    history: &mut Vec<ChatMessage>,
    compaction: &CompactionConfig,
) -> bool {
    // Load IDs from DB — messages were persisted before this call.
    let stored_ids = match db::sessions::get_session_message_ids(self.db(), session_id).await {
        Ok(ids) => ids,
        Err(e) => {
            tracing::warn!(error = %e, "Failed to load message IDs for compaction");
            return false;
        }
    };

    // Build parallel IDs matching the in-memory history structure.
    let session = match db::sessions::get_session(self.db(), session_id).await {
        Ok(s) => s,
        Err(_) => return false,
    };

    let mut parallel_ids = Vec::with_capacity(history.len());
    if session.compaction_summary.is_some() {
        parallel_ids.push(String::new());
    }
    let cursor = session.compaction_cursor_id.filter(|c| !c.is_empty());
    let mut include = cursor.is_none();
    for id in &stored_ids {
        if !include {
            include = Some(id.clone()) == cursor;
            continue;
        }
        parallel_ids.push(id.clone());
    }

    // Safety check: if IDs don't match history, log and fall back to Phase 1 only.
    if parallel_ids.len() != history.len() {
        tracing::error!(
            history_len = history.len(),
            ids_len = parallel_ids.len(),
            "compaction ID mismatch — falling back to Phase 1 masking only",
        );
        let keep_start = find_current_turn_start(history);
        *history = mask_tool_interactions(history, keep_start, compaction.mask_preview_chars);
        return false;
    }

    self.run_compaction(session_id, history, &parallel_ids, compaction)
        .await
}
```

- [ ] **Step 4: Run `just ci`**

Run: `just ci` Expected: All tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/chat/compaction.rs src/chat/session.rs
git commit -m "refactor: unify compaction paths into run_compaction with ID safety checks"
```

---

### Task 4: Update the live regression test

The existing `out_of_sync_live.rs` test sends a fresh sumo quiz request. It doesn't
reproduce the bug because `compact_if_needed` (called at the start of `chat.chat()`) has
correct IDs. The bug only triggers during the tool loop when Phase 2 fires
mid-iteration.

We can't easily force Phase 2 to trigger in a live test. But we CAN verify the
defense-in-depth: the empty-cursor guard and the ID mismatch detection.

**Files:**

- Modify: `tests/providers/out_of_sync_live.rs`

- [ ] **Step 1: Add integration test for the empty cursor guard**

In `tests/providers/out_of_sync_live.rs`, add a test that poisons the DB with an empty
cursor, then verifies `chat.chat()` still works (the guard in Task 1 saves us):

```rust
/// Regression: an empty compaction_cursor_id must not cause all messages
/// to be dropped. This is the defense-in-depth guard from Task 1.
#[tokio::test]
async fn empty_cursor_does_not_drop_messages() {
    let _obs = ghost::observability::init_for_live_tests()
        .expect("init live test observability");
    let env = common::live_test_database("empty_cursor").await;
    let session_id = env
        .session_with_messages(&[
            ("user", "What is 2+2?"),
            ("assistant", "4"),
        ])
        .await;

    // Poison: set a summary with an empty cursor — the exact bug state.
    ghost::db::sessions::update_compaction(
        &env.db,
        &session_id,
        "## Task\nThe user asked about math.",
        "",  // empty cursor — the bug
    )
    .await
    .expect("poison compaction");

    let chat = env.chat();
    let (result, _meta) = chat
        .chat(&session_id, "What is 3+3?", None, None)
        .await
        .expect("chat should succeed despite poisoned cursor");

    // The model must see both old messages AND the new one — not just
    // the summary. If the guard failed, it would only see the summary
    // and might not know to answer "6".
    assert!(
        !result.message.is_empty(),
        "response should not be empty",
    );
}
```

- [ ] **Step 2: Keep the existing `out_of_sync_reproduction` test as a smoke test**

The existing test verifies that a 209-message multi-topic session doesn't confuse the
model. Keep it as-is — it's still valuable as a general long-context regression test.

- [ ] **Step 3: Run all tests**

Run: `just ci && cargo test --features live-tests-llms out_of_sync -- --nocapture`
Expected: All pass.

- [ ] **Step 4: Commit**

```bash
git add tests/providers/out_of_sync_live.rs
git commit -m "test: add regression test for empty compaction cursor guard"
```
