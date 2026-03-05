# Session Event Bus — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this
> plan task-by-task.

**Goal:** Replace the two separate background-task notification mechanisms (completion
watcher + agent watcher) with a single session event bus.

**Architecture:** One mpsc channel carries `SessionEvent`s from producers (shell
background tasks, agent runner) to a single consumer (event handler) that injects system
messages, resolves session type (GHOST vs coding), triggers continuation chat turns, and
sends responses to Discord.

**Tech Stack:** `tokio::sync::mpsc`, existing SQLite queries, `SessionChat`.

---

### Task 1: Create `src/events.rs`

**Files:**

- Create: `src/events.rs`
- Modify: `src/main.rs` (add `pub mod events;`)

**Step 1: Write the module**

```rust
// src/events.rs
use tokio::sync::mpsc;

use crate::chat::RunMetadata;

/// A request to deliver a system message to a session and trigger a
/// continuation chat turn.
#[derive(Debug, Clone)]
pub struct SessionEvent {
    /// Target session ID
    pub session_id: String,
    /// System message to inject before triggering continuation
    pub system_message: String,
    /// Optional metadata for Discord presentation
    pub discord: Option<DiscordPayload>,
}

#[derive(Debug, Clone)]
pub struct DiscordPayload {
    /// Agent name + metadata for summary embed
    pub agent_name: Option<String>,
    pub agent_metadata: Option<RunMetadata>,
    pub agent_findings: Option<String>,
}

pub type SessionEventSender = mpsc::UnboundedSender<SessionEvent>;
pub type SessionEventReceiver = mpsc::UnboundedReceiver<SessionEvent>;

pub fn channel() -> (SessionEventSender, SessionEventReceiver) {
    mpsc::unbounded_channel()
}
```

Note: `DiscordPayload` carries raw agent info rather than pre-formatted text so the
consumer can call `format_agent_summary` (which lives in `interfaces::discord`). This
avoids the events module depending on Discord formatting.

**Step 2: Add `pub mod events;` to `src/main.rs`**

**Step 3: Verify it compiles**

Run: `cargo check`

**Step 4: Commit**

```
git add src/events.rs src/main.rs
git commit -m "feat: add session event bus types (src/events.rs)"
```

---

### Task 2: Add DB query for coding session lookup by chat session ID

**Files:**

- Modify: `src/db/coding_sessions.rs`

**Step 1: Write the query**

Add to `src/db/coding_sessions.rs`:

```rust
/// Look up an active coding session by its chat session ID.
/// Returns `(working_dir, channel_id)` if found.
pub async fn get_coding_session_for_chat_session(
    db: &SqlitePool,
    session_id: &str,
) -> Result<Option<(String, Option<String>)>, DatabaseError> {
    sqlx::query_as::<_, (String, Option<String>)>(
        "SELECT working_dir, channel_id FROM coding_sessions
         WHERE session_id = ? AND status = 'active'
         LIMIT 1",
    )
    .bind(session_id)
    .fetch_optional(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "coding_sessions",
        operation: "get_for_chat_session",
        source,
    })
}
```

**Step 2: Verify it compiles**

Run: `cargo check`

**Step 3: Commit**

```
git commit -am "feat: add coding session lookup by chat session ID"
```

---

### Task 3: Create `src/daemon/event_handler.rs`

**Files:**

- Create: `src/daemon/event_handler.rs`
- Modify: `src/daemon/mod.rs`

This is the single consumer that replaces both `completion_watcher.rs` and
`agents/watcher.rs`. It receives `SessionEvent`s and handles delivery.

**Step 1: Write the event handler**

```rust
// src/daemon/event_handler.rs
use std::sync::Arc;
use std::time::Duration;

use tokio::task::JoinHandle;

use crate::chat::SessionChat;
use crate::coding;
use crate::db;
use crate::db::GhostDb;
use crate::events::SessionEventReceiver;
use crate::interfaces::discord::DiscordSender;

const IDLE_POLL_INTERVAL: Duration = Duration::from_secs(2);
const MAX_IDLE_POLLS: usize = 30;

pub fn spawn_event_handler(
    mut rx: SessionEventReceiver,
    session_chat: Arc<SessionChat>,
    discord_sender: Option<Arc<DiscordSender>>,
    db: GhostDb,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) -> JoinHandle<()> {
    logfire::info!("event handler started");

    tokio::spawn(async move {
        loop {
            tokio::select! {
                Some(event) = rx.recv() => {
                    handle_event(
                        event,
                        &session_chat,
                        discord_sender.as_deref(),
                        &db,
                    ).await;
                }
                _ = shutdown.changed() => {
                    logfire::info!("event handler shutting down");
                    break;
                }
            }
        }
    })
}

async fn handle_event(
    event: crate::events::SessionEvent,
    session_chat: &SessionChat,
    discord_sender: Option<&DiscordSender>,
    db: &GhostDb,
) {
    let session_id = &event.session_id;

    logfire::info!(
        "session event received",
        session_id = session_id.clone(),
    );

    // Wait for the session to be idle before triggering continuation
    if !wait_for_idle(db, session_id).await {
        logfire::warn!(
            "session not idle after max polls, triggering anyway",
            session_id = session_id.clone(),
        );
    }

    // Check if this is a coding session
    let coding_ctx = db::coding_sessions::get_coding_session_for_chat_session(
        db, session_id,
    )
    .await
    .ok()
    .flatten();

    // Resolve Discord channel: interface_sessions first, coding_sessions fallback
    let discord_channel_id = resolve_discord_channel(db, session_id, &coding_ctx).await;

    // Send optional agent summary embed before the chat turn
    if let Some(sender) = discord_sender
        && let Some(channel_id) = discord_channel_id
        && let Some(ref discord_payload) = event.discord
        && let Some(ref agent_name) = discord_payload.agent_name
        && let Some(ref metadata) = discord_payload.agent_metadata
    {
        let summary = crate::interfaces::discord::ui_events::format_agent_summary(
            agent_name,
            metadata,
            discord_payload.agent_findings.as_deref(),
        );
        let _ = sender
            .send_compact_container(channel_id, &summary, None)
            .await;
    }

    // Trigger continuation chat turn
    let trigger = "[system] Background task completed.";
    let chat_result = if let Some((ref working_dir, _)) = coding_ctx {
        let system_prompt = coding::prompt::build_coding_prompt(
            // Need config access — get from session_chat
            session_chat.config(),
            std::path::Path::new(working_dir),
        );
        session_chat
            .chat_coding(session_id, trigger, &system_prompt, None)
            .await
    } else {
        session_chat.chat(session_id, trigger, None).await
    };

    match chat_result {
        Ok((result, _metadata)) => {
            if let Some(sender) = discord_sender
                && let Some(channel_id) = discord_channel_id
                && let Err(e) = sender.send_to_channel(channel_id, &result.message).await
            {
                logfire::error!(
                    "failed to send event response to Discord",
                    error = e.to_string(),
                );
            }
        }
        Err(e) => {
            logfire::error!(
                "failed to trigger chat turn after session event",
                session_id = session_id.clone(),
                error = e.to_string(),
            );
        }
    }
}

/// Resolve Discord channel ID: check interface_sessions first, then
/// coding_sessions as fallback.
async fn resolve_discord_channel(
    db: &GhostDb,
    session_id: &str,
    coding_ctx: &Option<(String, Option<String>)>,
) -> Option<u64> {
    // Try interface_sessions first (normal GHOST sessions)
    let channel = db::sessions::get_interface_for_session(db, session_id)
        .await
        .ok()
        .flatten();

    if let Some(channel_id) = channel.as_deref().and_then(parse_discord_channel_id) {
        return Some(channel_id);
    }

    // Fallback: coding session's channel_id
    if let Some((_, Some(ref channel_str))) = coding_ctx {
        return channel_str.parse().ok();
    }

    None
}

/// Poll until the session's latest message is idle (no pending tool calls).
async fn wait_for_idle(db: &GhostDb, session_id: &str) -> bool {
    for _ in 0..MAX_IDLE_POLLS {
        let messages = db::sessions::list_messages_by_session(db, session_id)
            .await
            .unwrap_or_default();

        if let Some(last) = messages.last() {
            let has_tool_calls = last.tool_calls_parsed().is_some_and(|tc| !tc.is_empty());

            if last.role == "assistant" && !has_tool_calls {
                return true;
            }

            // Accept system messages from background tasks as idle
            if last.role == "system" {
                return true;
            }
        }

        tokio::time::sleep(IDLE_POLL_INTERVAL).await;
    }

    false
}

fn parse_discord_channel_id(interface_key: &str) -> Option<u64> {
    interface_key
        .strip_prefix("discord:channel:")
        .and_then(|id| id.parse().ok())
}
```

Note: the `wait_for_idle` system message check is broadened from the original (which
only matched `[shell-command completed]`) to accept any system message. This is correct
because the producer has already injected the system message before sending the event.

**Step 2: Check if `SessionChat` exposes `config()`**

It may need a `pub fn config(&self) -> &Config` accessor. If not present, add it.

**Step 3: Update `src/daemon/mod.rs`**

Replace:

```rust
pub mod completion_watcher;
```

With:

```rust
pub mod event_handler;
```

**Step 4: Verify it compiles**

Run: `cargo check`

Expect: errors from `daemon/run.rs` (still references old watchers). That's expected —
we fix it in Task 6.

**Step 5: Commit**

```
git add src/daemon/event_handler.rs src/daemon/mod.rs
git commit -m "feat: add unified session event handler"
```

---

### Task 4: Wire `SessionEventSender` into `AgentRunner`

**Files:**

- Modify: `src/agents/runner.rs`

**Step 1: Add `event_tx` field to `AgentRunner` and `BackgroundTask`**

In `AgentRunner`:

```rust
pub struct AgentRunner {
    db: GhostDb,
    config: Config,
    handles: Arc<Mutex<HashMap<String, AgentHandle>>>,
    event_tx: Option<crate::events::SessionEventSender>,
}
```

Update `new()` to accept and store the sender:

```rust
pub fn new(
    db: GhostDb,
    config: Config,
    event_tx: Option<crate::events::SessionEventSender>,
) -> Self {
    Self { db, config, handles: Arc::new(Mutex::new(HashMap::new())), event_tx }
}
```

Add `event_tx` and `handles` to `BackgroundTask`:

```rust
struct BackgroundTask {
    // ... existing fields ...
    event_tx: Option<crate::events::SessionEventSender>,
    handles: Arc<Mutex<HashMap<String, AgentHandle>>>,
}
```

**Step 2: Pass `event_tx` and `handles` when constructing `BackgroundTask`**

In `run_in_background` and `resume_in_background`, add `event_tx: self.event_tx.clone()`
and `handles: Arc::clone(&self.handles)` to the `BackgroundTask` construction.

**Step 3: Update `finish_background` to send event and clean up handle**

```rust
async fn finish_background(task: BackgroundTask, result: Result<AgentResult, AgentError>) {
    let (status, transcript, metadata) = match result {
        Ok(agent_result) => {
            spawn_children_inner(
                agent_result.spawns,
                &task.db,
                &task.config,
                &task.agent_session_id,
                task.depth,
            );
            ("ok", agent_result.findings.clone(), Some(agent_result.metadata.clone()))
        }
        Err(e) => {
            logfire::error!(
                "agent failed",
                agent_name = task.agent_name.clone(),
                error = e.to_string(),
            );
            let partial = last_assistant_message(&task.db, &task.agent_session_id).await;
            ("failed", partial, None)
        }
    };

    // Persist run record
    if let Err(e) = db::agent_runs::finish_run(
        &task.db, &task.run_id, status, &transcript,
    ).await {
        logfire::error!("failed to finish agent run", error = e.to_string());
    }

    // Store metadata in handle before removing it
    if let Some(ref metadata) = metadata {
        // Keep the metadata_slot update for status() calls that happen
        // before the handle is removed
        *task.metadata_slot.lock().await = Some(metadata.clone());
    }

    // Send session event to parent (if there is a parent)
    if let Some(ref parent_id) = task.parent_session_id {
        let system_message = format!(
            "[agent:{} completed]\n\n{}",
            task.agent_name, transcript,
        );

        // Inject system message into parent session
        if let Err(e) = db::sessions::create_message(
            &task.db, parent_id, "system", &system_message,
        ).await {
            logfire::error!(
                "failed to inject agent findings into parent session",
                error = e.to_string(),
            );
        }

        // Send event for continuation
        if let Some(ref tx) = task.event_tx {
            let discord = metadata.map(|m| crate::events::DiscordPayload {
                agent_name: Some(task.agent_name.clone()),
                agent_metadata: Some(m),
                agent_findings: Some(transcript.clone()),
            });
            let _ = tx.send(crate::events::SessionEvent {
                session_id: parent_id.clone(),
                system_message,
                discord,
            });
        }
    }

    // Clean up handle
    task.handles.lock().await.remove(&task.agent_session_id);

    logfire::info!(
        "agent finished",
        agent_name = task.agent_name.clone(),
        status = status,
    );
}
```

**Step 4: Remove `take_completed`**

Delete the `take_completed` method from `AgentRunner`. It was only used by the agent
watcher.

**Step 5: Verify it compiles**

Run: `cargo check`

Expect: errors from callers of `AgentRunner::new()` (now takes 3 args) and from
`watcher.rs` (calls `take_completed`). That's expected — fixed in later tasks.

**Step 6: Commit**

```
git commit -am "feat: AgentRunner sends SessionEvent on completion"
```

---

### Task 5: Wire `SessionEventSender` into shell tool

**Files:**

- Modify: `src/tools/context.rs`
- Modify: `src/tools/shell.rs`
- Modify: `src/chat/session.rs`

**Step 1: Replace `completion_tx` with `event_tx` in `ToolContext`**

In `src/tools/context.rs`, change:

```rust
pub completion_tx: Option<CompletionSender>,
```

to:

```rust
pub event_tx: Option<crate::events::SessionEventSender>,
```

**Step 2: Update `SessionChat`**

In `src/chat/session.rs`, replace:

- Field: `completion_tx: Option<crate::completion::CompletionSender>` →
  `event_tx: Option<crate::events::SessionEventSender>`
- Method: `with_completion_sender` → `with_event_sender`
- In `execute_single_tool` (line ~294): `completion_tx: self.completion_tx.clone()` →
  `event_tx: self.event_tx.clone()`

**Step 3: Update shell tool background path**

In `src/tools/shell.rs` (~line 103), change:

```rust
let completion_tx = ctx.completion_tx.clone();
```

to:

```rust
let event_tx = ctx.event_tx.clone();
```

And replace the event send (~lines 139-144):

```rust
if let Some(ref tx) = event_tx {
    let _ = tx.send(crate::events::SessionEvent {
        session_id: session_id.clone(),
        system_message: msg.clone(),
        discord: None,
    });
}
```

Note: the shell tool already injects the system message into DB (line 130). The event
just triggers continuation. The `system_message` field on the event is for reference —
the DB write is the source of truth.

**Step 4: Fix all `completion_tx: None` in test contexts**

Grep for `completion_tx: None` in `src/tools/` (read_file.rs, write_file.rs,
file_edit.rs, shell.rs test contexts). Change all to `event_tx: None`.

**Step 5: Verify it compiles**

Run: `cargo check`

**Step 6: Commit**

```
git commit -am "refactor: replace completion_tx with event_tx in ToolContext and shell"
```

---

### Task 6: Wire up daemon boot and delete old code

**Files:**

- Modify: `src/daemon/run.rs`
- Modify: `src/daemon/mod.rs`
- Modify: `src/agents/mod.rs`
- Delete: `src/completion.rs`
- Delete: `src/daemon/completion_watcher.rs`
- Delete: `src/agents/watcher.rs`

**Step 1: Update `daemon/run.rs` boot function**

Replace the completion channel + agent runner + watcher wiring block (~lines 111-156)
with:

```rust
// Create session event channel
let (event_tx, event_rx) = crate::events::channel();

// Create agent runner with event sender
let agent_runner = Arc::new(AgentRunner::new(
    db.clone(),
    config.clone(),
    Some(event_tx.clone()),
));

// ... scheduler spawn stays the same ...

let session_chat = Arc::new(
    SessionChat::from_config(db.clone(), config.clone())?
        .with_agent_runner(Arc::clone(&agent_runner))
        .with_event_sender(event_tx),
);

let discord_result = discord::start_discord(&config, session_chat.clone(), db.clone()).await?;

let discord_sender_arc = discord_result
    .as_ref()
    .map(|(sender, _)| Arc::new(sender.clone()));

// Spawn unified event handler (replaces agent_watcher + completion_watcher)
let event_handler_handle = super::event_handler::spawn_event_handler(
    event_rx,
    Arc::clone(&session_chat),
    discord_sender_arc,
    db.clone(),
    shutdown_rx.clone(),
);
```

**Step 2: Update `BootResult` type and `run()` shutdown**

Simplify `BootResult` — remove the `Option<JoinHandle<()>>` for agent watcher and the
separate completion watcher handle. Replace with single `event_handler_handle`.

Update shutdown in `run()`:

```rust
let _ = shutdown_tx.send(true);
let _ = watcher_handle.await;
let _ = scheduler_handle.await;
let _ = event_handler_handle.await;
```

**Step 3: Update `daemon/mod.rs`**

Remove `pub mod completion_watcher;`. The `event_handler` module was already added in
Task 3.

**Step 4: Delete old files**

- Delete `src/completion.rs`
- Delete `src/daemon/completion_watcher.rs`
- Delete `src/agents/watcher.rs`

**Step 5: Update `src/agents/mod.rs`**

Remove `pub mod watcher;`.

**Step 6: Remove `pub mod completion;` from `src/main.rs`**

The `events` module was added in Task 1. Remove the old `completion` module declaration.

**Step 7: Verify it compiles**

Run: `cargo check`

Fix any remaining references to old types (`CompletionSender`, `CompletionEvent`,
`completion_tx`, `spawn_agent_watcher`, `spawn_completion_watcher`, `take_completed`).

**Step 8: Commit**

```
git commit -am "refactor: replace watchers with unified session event handler"
```

---

### Task 7: Update test helper

**Files:**

- Modify: `tests/common.rs`

**Step 1: Update `chat_with_completion_watcher`**

Rename to `chat_with_event_handler` (or keep name for minimal churn — your call). Update
internals:

```rust
pub fn chat_with_event_handler(
    &self,
) -> (Arc<ghost::chat::SessionChat>, tokio::task::JoinHandle<()>) {
    let (event_tx, event_rx) = ghost::events::channel();
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    std::mem::forget(shutdown_tx);

    let session_chat = Arc::new(
        ghost::chat::SessionChat::from_config(self.db.clone(), self.config.clone())
            .expect("build session chat")
            .with_agent_runner(Arc::clone(&self.agent_runner))
            .with_event_sender(event_tx),
    );

    let handler_handle = ghost::daemon::event_handler::spawn_event_handler(
        event_rx,
        Arc::clone(&session_chat),
        None,
        self.db.clone(),
        shutdown_rx,
    );

    (session_chat, handler_handle)
}
```

**Step 2: Update callers**

Grep `chat_with_completion_watcher` in `tests/`. Update all call sites to use the new
name (if renamed).

**Step 3: Update `AgentRunner::new` call in test helper**

The test helper likely constructs an `AgentRunner`. Update to pass `None` as event_tx
(or wire it through if tests need agent completion events).

**Step 4: Verify tests compile and pass**

Run: `just ci`

**Step 5: Commit**

```
git commit -am "test: update test helpers for session event bus"
```

---

### Task 8: Final cleanup and verification

**Files:**

- All modified files

**Step 1: Grep for stale references**

Search for any remaining references to the old types:

- `CompletionEvent`
- `CompletionSender`
- `CompletionReceiver`
- `completion_tx`
- `completion_watcher`
- `agent_watcher`
- `take_completed`
- `spawn_agent_watcher`
- `spawn_completion_watcher`

Fix any that remain.

**Step 2: Run full CI**

Run: `just ci`

All format, check, clippy, and tests must pass.

**Step 3: Commit any fixes**

```
git commit -am "chore: clean up stale references to old watcher code"
```

**Step 4: Update design doc status**

Change `docs/plans/2026-03-05-session-event-bus.md` status from DRAFT to IMPLEMENTED.

```
git commit -am "docs: mark session event bus design as implemented"
```
