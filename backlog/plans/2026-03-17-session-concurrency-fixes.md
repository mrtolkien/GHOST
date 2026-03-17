# Session Concurrency Fixes — Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if
> subagents available) or superpowers:executing-plans to implement this plan. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix two concurrency bugs: (1) race condition allowing duplicate tool loops for
the same session, and (2) steer messages silently lost when the tool loop exits on
EndTurn.

**Architecture:** Add an atomic check-and-insert guard to `chat_with_images()` /
`chat_coding()` using `DashMap::entry()`. Add interrupt draining to the EndTurn path in
the tool loop. Callers (event handler, Discord bot) handle the new `SessionBusy` error.

**Tech Stack:** Rust, DashMap, tokio mpsc channels.

---

## Root Cause Analysis

### Bug 1: Duplicate responses (race condition)

The event handler (`event_handler.rs`) calls `session_chat.chat()` / `.chat_coding()`
without checking `active_sessions`. These methods use `DashMap::insert()` which silently
overwrites any existing entry. When a Discord message and a background task completion
arrive at nearly the same time, both start independent tool loops on the same session,
producing duplicate responses.

Timeline from production (2026-03-16T20:44):

- T+0s: Discord handler starts processing `.lollycherry`'s message (trace `c64d`)
- T+4s: Event handler's `wait_for_idle()` returns true (system message is last in DB)
- T+4s: Event handler calls `chat()` → both run concurrently → two identical responses

### Bug 2: Silently dropped messages (steer lost on EndTurn)

When a Discord message arrives while a tool loop is running, the Discord handler sends
`Interrupt::Steer` via the interrupt channel. `drain_interrupts()` is called between
tool iterations (after tool execution), where it persists the steer message to DB and
appends it to history.

**Problem:** When the model returns `EndTurn` (no tool calls), the tool loop exits
without calling `drain_interrupts()`. Any steer messages buffered in the channel are
silently dropped — never persisted to DB, never seen by the model.

---

## File Map

| File                            | Action | Responsibility                                                       |
| ------------------------------- | ------ | -------------------------------------------------------------------- |
| `src/chat/types.rs`             | Modify | Add `SessionBusy` variant to `ChatError`                             |
| `src/chat/session.rs`           | Modify | Atomic check-and-insert in `chat_with_images()`, `chat_coding()`     |
| `src/chat/tool_loop.rs`         | Modify | Drain interrupts on EndTurn, continue loop if steer messages pending |
| `src/daemon/event_handler.rs`   | Modify | Handle `SessionBusy` gracefully (log + skip)                         |
| `src/interfaces/discord/bot.rs` | Modify | Handle `SessionBusy` from `chat_with_images()` (steer fallback)      |

---

## Chunk 1: SessionBusy guard and caller handling

### Task 1: Add `SessionBusy` error variant and atomic session guard

**Files:**

- Modify: `src/chat/types.rs:27-46`
- Modify: `src/chat/session.rs:162-236` (`chat_with_images`)
- Modify: `src/chat/session.rs:238-289` (`chat_coding`)

- [ ] **Step 1: Add `SessionBusy` variant to `ChatError`**

In `src/chat/types.rs`, add after the existing variants:

```rust
#[error("session '{session_id}' already has an active tool loop")]
SessionBusy { session_id: String },
```

- [ ] **Step 2: Make `chat_with_images()` use atomic check-and-insert**

In `src/chat/session.rs`, the current flow in `chat_with_images()` is:

1. Parse session, DB reads, write user message (lines 170-202)
2. Load history, compact (lines 204-206)
3. Create interrupt channel and insert into `active_sessions` (lines 217-218)
4. Run tool loop (lines 220-232)
5. Remove from `active_sessions` (line 234)

**Reorder** so the atomic guard happens BEFORE writing the user message to DB. This
ensures that if `SessionBusy` is returned, no side effects occurred:

```rust
pub async fn chat_with_images(
    &self,
    session_id: &str,
    user_message: &str,
    images: Option<Vec<ContentBlock>>,
    channel_id: Option<String>,
    event_tx: Option<&EventSender>,
) -> Result<(ChatResult, RunMetadata), ChatError> {
    let session_thing = parse_session_thing(session_id)?;
    db::sessions::get_session(&self.db, &session_thing).await?;
    db::sessions::update_activity(&self.db, &session_thing).await?;

    // Atomic session guard — prevent concurrent tool loops.
    let (int_tx, int_rx) = super::interrupt::channel();
    {
        use dashmap::mapref::entry::Entry;
        match self.active_sessions.entry(session_id.to_string()) {
            Entry::Occupied(_) => {
                return Err(ChatError::SessionBusy {
                    session_id: session_id.to_string(),
                });
            }
            Entry::Vacant(entry) => {
                entry.insert(int_tx);
            }
        }
    }

    // From here, active_sessions is held. Always remove on exit.
    let result = self
        .chat_with_images_inner(
            session_id,
            &session_thing,
            user_message,
            images,
            channel_id,
            event_tx,
            int_rx,
        )
        .await;

    self.active_sessions.remove(session_id);
    result
}
```

The exact refactoring is up to the implementer. The key invariant:
**`active_sessions.entry()` must be checked BEFORE writing the user message to DB.** The
simplest approach is to move the interrupt channel + entry check to right after
`update_activity`, then keep the rest of the method body as-is but with the insert line
removed. No need to extract an inner method if it adds complexity — just move the lines.

- [ ] **Step 3: Apply the same pattern to `chat_coding()`**

Same change: move the interrupt channel creation and `active_sessions.entry()` check to
before the user message DB write (line 252). Remove the unconditional `insert` at
line 271.

**Note:** `chat()` at line 151 is a thin wrapper that delegates to `chat_with_images()`,
so it is automatically covered — no guard needed on `chat()` itself.

- [ ] **Step 4: Verify it compiles**

Run: `cargo check 2>&1 | head -40`

Note: this will show errors in `event_handler.rs` and `bot.rs` where `SessionBusy` is
not yet handled. That's expected — we fix those in Task 2.

- [ ] **Step 5: Commit**

```
feat: add atomic session guard to prevent concurrent tool loops
```

---

### Task 2: Handle `SessionBusy` in callers

**Files:**

- Modify: `src/daemon/event_handler.rs:91-118`
- Modify: `src/interfaces/discord/bot.rs:322-331` (`handle_message` result match)
- Modify: `src/interfaces/discord/bot.rs:454-499` (`handle_coding_message` result match)

- [ ] **Step 1: Event handler — skip on `SessionBusy`**

In `event_handler.rs`, the `chat_result` match at lines 91-118 calls `.chat()` or
`.chat_coding()`. Wrap the match to handle `SessionBusy`:

```rust
let chat_result = match detect_coding_session(db, session_chat, session_id).await {
    Some((working_dir, system_prompt)) => {
        // ... existing logging ...
        session_chat
            .chat_coding(session_id, trigger, &system_prompt, &working_dir, channel_id_str, None)
            .await
    }
    None => {
        // ... existing logging ...
        session_chat
            .chat(session_id, trigger, channel_id_str, None)
            .await
    }
};

// If the session is already being handled (e.g. by a Discord message),
// the running tool loop will see the background task's system message
// in its history. No need to trigger a separate response.
if matches!(&chat_result, Err(ChatError::SessionBusy { .. })) {
    logfire::info!(
        "session already active, skipping continuation",
        session_id = session_id.clone(),
    );
    return;
}
```

Add `use crate::chat::types::ChatError;` to the imports if not already present.

- [ ] **Step 2: Discord handler — steer on `SessionBusy`**

In `bot.rs`, the `chat_result` match at line 337 currently handles `Ok` and `Err`. Add
handling for `SessionBusy`:

```rust
match chat_result {
    Ok((result, metadata)) => {
        // ... existing success handling ...
    }
    Err(ChatError::SessionBusy { .. }) => {
        // TOCTOU race: session became active between the check at
        // line 301 and chat_with_images(). Steer the running loop.
        if let Some(tx) = self.active_sessions.get(&session_id) {
            let _ = tx.send(crate::chat::interrupt::Interrupt::Steer {
                message: full_content,
            });
        }
    }
    Err(e) => {
        // ... existing error handling ...
    }
}
```

Add `use crate::chat::types::ChatError;` to imports if needed.

- [ ] **Step 3: Discord handler — steer on `SessionBusy` in `handle_coding_message`**

The `handle_coding_message` method (line 390) has the same steer guard at line 423 and
the same TOCTOU race. Its result match at line 454 currently routes all errors to a
generic `Err(e)` arm that sends `format!("Error: {e}")` to Discord — which would expose
the `SessionBusy` error message to the user.

Add the same `SessionBusy` handling before the generic `Err(e)` arm at line 486:

```rust
match chat_result {
    Ok((result, metadata)) => {
        // ... existing success handling ...
    }
    Err(ChatError::SessionBusy { .. }) => {
        // TOCTOU race: session became active between the check at
        // line 423 and chat_coding(). Steer the running loop.
        if let Some(tx) = self.active_sessions.get(session_id) {
            let _ = tx.send(crate::chat::interrupt::Interrupt::Steer {
                message: full_content,
            });
        }
    }
    Err(e) => {
        // ... existing error handling ...
    }
}
```

- [ ] **Step 4: Verify it compiles and passes clippy**

Run: `cargo clippy 2>&1 | head -40`

- [ ] **Step 5: Commit**

```
feat: handle SessionBusy in event handler and Discord bot
```

---

## Chunk 2: Drain interrupts on EndTurn

### Task 3: Drain pending interrupts before exiting on EndTurn

**Files:**

- Modify: `src/chat/tool_loop.rs:411-490`

- [ ] **Step 1: Add interrupt drain after `on_end_turn` in the EndTurn branch**

The current EndTurn branch (lines 411-490) calls `on_end_turn()` and then returns.
Insert an interrupt drain between the two. If steer messages were received during the
final LLM call, persist them and continue the loop so the model can respond.

After the `on_end_turn` call (line 479) and before the `logfire::info!` / return block
(line 480), insert:

```rust
// Drain pending interrupts before exiting. If a user message
// arrived during the final LLM call, persist it and continue
// so the model sees and responds to it.
if let Some(ref mut rx) = interrupt_rx {
    let pre_drain_len = history.len();
    match drain_interrupts(rx, history, session_chat.db(), session_id).await? {
        InterruptAction::Stop => {
            // User sent /stop — return current result.
            metadata.iterations = iterations;
            metadata.duration = started_at.elapsed();
            return Ok((result, metadata));
        }
        InterruptAction::Continue => {
            if history.len() > pre_drain_len {
                // New user messages were injected. Push the
                // assistant's EndTurn response into history
                // and continue so the model responds to them.
                history.push(ChatMessage {
                    role: Role::Assistant,
                    content: response.content,
                });
                // Clear last_result so a MaxIterations fallback
                // won't return the pre-steer response.
                last_result = None;
                iterations += 1;
                continue;
            }
        }
    }
}
```

**Important:** This block must go AFTER the empty-response check and the progress gate
check (which both `continue`), and AFTER `on_end_turn()` (which persists the assistant
message to DB), but BEFORE the final `return`.

The `response.content` is still available here because `extract_text_content()`,
`extract_tool_use_blocks()`, and `raw_output_to_values()` all borrow it.

- [ ] **Step 2: Verify it compiles**

Run: `cargo check 2>&1 | head -20`

- [ ] **Step 3: Run `just ci`**

Run: `just ci` Expected: all checks pass, all existing tests pass.

- [ ] **Step 4: Commit**

```
fix: drain pending steer interrupts on EndTurn to prevent message loss
```

---

## Chunk 3: Testing

### Task 4: Unit tests

**Files:**

- Modify: `src/chat/types.rs` (or `src/chat/session.rs` test module)

- [ ] **Step 1: Test that `SessionBusy` is returned when session is already active**

Read the `@testing` skill before writing any test.

This test should:

1. Create a `SessionChat` (using test helpers from `tests/common.rs` — `test_config`,
   `test_database`, `MockProvider`)
2. Pre-insert a session into `active_sessions` via
   `session_chat.active_sessions().insert("test-session", tx)`
3. Call `session_chat.chat("test-session", "hello", None, None).await`
4. Assert the result is `Err(ChatError::SessionBusy { .. })`
5. Verify no user message was written to DB for that session

Place this test in `src/chat/session.rs`'s `#[cfg(test)]` module, or in an existing
integration test file if the `SessionChat` setup requires resources that unit tests
can't provide.

If creating a `SessionChat` in a unit test is too involved (requires provider, DB,
etc.), this can be an integration test in `tests/`. Use `test_config()` and
`test_database()` from `tests/common.rs`.

- [ ] **Step 2: Test that interrupts are drained on EndTurn**

This test verifies the tool loop fix. It requires:

1. A `MockProvider` that returns `EndTurn` on the first call, then `EndTurn` again on
   the second
2. An interrupt channel where a `Steer` message is sent after the first LLM call starts
   but before the loop checks interrupts
3. Assert that the steered message was persisted to DB
4. Assert that the loop made 2 LLM calls (not 1)

This is trickier to set up because timing matters. A simpler approach: pre-load a
`Steer` into the interrupt channel BEFORE the tool loop starts (since `drain_interrupts`
uses `try_recv`, it will find the message immediately). Then:

1. Mock provider returns `EndTurn` with some text
2. Steer message is pre-loaded in the channel
3. After the first EndTurn, `drain_interrupts` finds the steer → continues loop
4. Mock provider returns `EndTurn` again on second call
5. No more interrupts → exits
6. Assert: 2 LLM calls were made, steer message is in DB

- [ ] **Step 3: Run `just ci`**

Run: `just ci` Expected: all tests pass including new ones.

- [ ] **Step 4: Commit**

```
test: session concurrency guard and EndTurn interrupt drain
```

---

## Verification Checklist

After all tasks are complete, verify these scenarios mentally or via test:

1. **Event handler + Discord handler race**: Event handler gets `SessionBusy`, logs and
   skips. Discord handler's tool loop sees the background task's system message in
   history. One response only.
2. **Discord message during final LLM call**: Steer is sent, `drain_interrupts` on
   EndTurn picks it up, persists to DB, model gets another turn. User sees a response to
   their message.
3. **Discord message during tool execution**: Existing behavior unchanged —
   `drain_interrupts` between iterations picks it up.
4. **No concurrent access**: Normal flow unchanged — `Entry::Vacant` path behaves
   identically to the old `insert`.
5. **TOCTOU in Discord handler**: Discord passes line 301 check, gets `SessionBusy` from
   `chat_with_images()`, steers the now-active session.
