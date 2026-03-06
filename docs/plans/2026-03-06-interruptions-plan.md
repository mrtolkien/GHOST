# Interruptions & Steering — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Let the OPERATOR send messages to a running tool loop (steering) and gracefully stop it (`/stop`), via Discord.

**Architecture:** An `mpsc` interrupt channel is created per tool loop invocation and registered in a shared `ActiveSessions` map. The Discord handler checks this map — if a session is active, it sends an `Interrupt` instead of starting a new `chat()`. The tool loop drains interrupts between tool iterations via `try_recv()`.

**Tech Stack:** Rust, tokio mpsc, dashmap, serenity (Discord)

**Design doc:** `docs/plans/2026-03-06-interruptions-design.md`

---

### Task 1: Add `dashmap` dependency

**Files:**
- Modify: `Cargo.toml`

**Step 1: Add dashmap to Cargo.toml**

Add `dashmap` to the `[dependencies]` section:

```toml
dashmap = "6"
```

**Step 2: Verify it compiles**

Run: `cargo check`
Expected: compiles with no errors

**Step 3: Commit**

```
git add Cargo.toml Cargo.lock
git commit -m "chore: add dashmap dependency for active session tracking"
```

---

### Task 2: Create `src/chat/interrupt.rs` and add `ChatStopReason::Stopped`

**Files:**
- Create: `src/chat/interrupt.rs`
- Modify: `src/chat/mod.rs:1-12`
- Modify: `src/chat/types.rs:13-18`

**Step 1: Create `src/chat/interrupt.rs`**

```rust
use std::sync::Arc;

use dashmap::DashMap;
use tokio::sync::mpsc;

/// Message sent into a running tool loop to steer or stop it.
#[derive(Debug)]
pub enum Interrupt {
    /// Inject a user message between tool calls.
    Steer { message: String },
    /// Stop the tool loop gracefully after the current tool finishes.
    Stop,
}

pub type InterruptSender = mpsc::UnboundedSender<Interrupt>;
pub type InterruptReceiver = mpsc::UnboundedReceiver<Interrupt>;

pub fn channel() -> (InterruptSender, InterruptReceiver) {
    mpsc::unbounded_channel()
}

/// Tracks which sessions have a running tool loop.
/// Key: session_id, Value: sender to interrupt that loop.
pub type ActiveSessions = Arc<DashMap<String, InterruptSender>>;
```

**Step 2: Export from `src/chat/mod.rs`**

Add `pub mod interrupt;` and re-export `ActiveSessions`:

```rust
mod compaction;
mod convert;
pub mod interrupt;
mod session;
mod tool_loop;
pub mod transcript;
mod types;

pub use interrupt::ActiveSessions;
pub use session::SessionChat;
pub use transcript::{extract_agent_findings, filter_transcript};
pub use types::{
    ChatError, ChatResult, ChatStopReason, EventSender, RunMetadata, ToolCallInfo, ToolLoopEvent,
};
```

**Step 3: Add `Stopped` variant to `ChatStopReason`**

In `src/chat/types.rs`, add `Stopped` to the enum:

```rust
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
pub enum ChatStopReason {
    EndTurn,
    MaxTokens,
    MaxIterations,
    Stopped,
}
```

**Step 4: Verify it compiles**

Run: `cargo check`
Expected: compiles (no callers use `Stopped` yet, and `ChatStopReason` is non-exhaustive in match arms that already have `_` or explicit variants)

**Step 5: Commit**

```
git add src/chat/interrupt.rs src/chat/mod.rs src/chat/types.rs
git commit -m "feat: add interrupt channel types and ChatStopReason::Stopped"
```

---

### Task 3: Wire `InterruptReceiver` into `run_tool_loop`

**Files:**
- Modify: `src/chat/tool_loop.rs:78-88` (signature), `~104` (top of loop), `~296-297` (after ToolUse post_tool_iteration)

**Step 1: Add the interrupt receiver parameter**

Change the `run_tool_loop` signature to accept an interrupt receiver:

```rust
pub(super) async fn run_tool_loop(
    session_chat: &SessionChat,
    session_id: &str,
    model: &str,
    max_iterations: usize,
    reasoning_effort: ReasoningEffort,
    handler: &mut (impl ToolLoopHandler + ?Sized),
    history: &mut Vec<ChatMessage>,
    event_tx: Option<&EventSender>,
    interrupt_rx: Option<&mut super::interrupt::InterruptReceiver>,
) -> Result<(ChatResult, RunMetadata), ChatError> {
```

**Step 2: Add interrupt drain logic after `post_tool_iteration`**

After the `handler.post_tool_iteration(...)` call inside the `StopReason::ToolUse` arm (around line 296), add the interrupt check. Add this helper function and enum at the top of the file:

```rust
use super::interrupt::{Interrupt, InterruptReceiver};

enum InterruptAction {
    Continue,
    Stop,
}

/// Drain all pending interrupts from the channel.
/// Steer messages are persisted to DB and appended to history.
/// Returns `Stop` if any `Interrupt::Stop` was received.
async fn drain_interrupts(
    rx: &mut InterruptReceiver,
    history: &mut Vec<ChatMessage>,
    db: &crate::db::GhostDb,
    session_id: &str,
) -> Result<InterruptAction, ChatError> {
    let mut action = InterruptAction::Continue;
    while let Ok(interrupt) = rx.try_recv() {
        match interrupt {
            Interrupt::Stop => {
                action = InterruptAction::Stop;
                // Don't break — drain remaining steers so they're persisted
            }
            Interrupt::Steer { message } => {
                crate::db::sessions::create_message(db, session_id, "user", &message).await?;
                history.push(ChatMessage {
                    role: Role::User,
                    content: vec![ContentBlock::Text { text: message }],
                });
            }
        }
    }
    Ok(action)
}
```

Then after `post_tool_iteration` in the ToolUse arm, add:

```rust
// Check for OPERATOR interrupts (steering messages or /stop)
if let Some(ref mut rx) = interrupt_rx {
    match drain_interrupts(rx, history, session_chat.db(), session_id).await? {
        InterruptAction::Continue => {}
        InterruptAction::Stop => {
            metadata.iterations = iterations;
            metadata.duration = started_at.elapsed();
            let fallback = last_result.unwrap_or(ChatResult {
                message: String::new(),
                stop_reason: ChatStopReason::Stopped,
            });
            return Ok((
                ChatResult {
                    stop_reason: ChatStopReason::Stopped,
                    ..fallback
                },
                metadata,
            ));
        }
    }
}
```

**Important**: `interrupt_rx` must be stored as a mutable local, not a reference parameter, because `&mut Option<&mut T>` is awkward. Use `mut interrupt_rx: Option<&mut InterruptReceiver>` in the signature so `ref mut` works on it inside the loop. Actually, since we use it across loop iterations, keep the parameter as `mut interrupt_rx: Option<InterruptReceiver>` (owned) instead of a reference. This avoids borrow checker issues with `&mut` across await points.

Updated signature:

```rust
pub(super) async fn run_tool_loop(
    session_chat: &SessionChat,
    session_id: &str,
    model: &str,
    max_iterations: usize,
    reasoning_effort: ReasoningEffort,
    handler: &mut (impl ToolLoopHandler + ?Sized),
    history: &mut Vec<ChatMessage>,
    event_tx: Option<&EventSender>,
    mut interrupt_rx: Option<InterruptReceiver>,
) -> Result<(ChatResult, RunMetadata), ChatError> {
```

And the check becomes:

```rust
if let Some(ref mut rx) = interrupt_rx {
    match drain_interrupts(rx, history, session_chat.db(), session_id).await? {
        // ...
    }
}
```

**Step 3: Verify it compiles**

Run: `cargo check`
Expected: Compile errors in callers of `run_tool_loop` (they don't pass the new param yet). That's expected — we fix callers in the next task.

**Step 4: Fix all callers in `session.rs`**

In `src/chat/session.rs`, update all four calls to `run_tool_loop` to pass `None` as the last argument:

```rust
run_tool_loop(
    self,
    session_id,
    &model,
    self.max_tool_iterations,
    effort,
    &mut handler,
    &mut history,
    event_tx,
    None, // interrupt_rx — wired in next task
)
.await
```

There are four call sites: `chat()`, `chat_coding()`, `run_agent()`, and `run_agent_with_history()`. Update all four.

**Step 5: Verify it compiles and tests pass**

Run: `just ci`
Expected: all green

**Step 6: Commit**

```
git add src/chat/tool_loop.rs src/chat/session.rs
git commit -m "feat: wire interrupt receiver into tool loop with drain logic"
```

---

### Task 4: Add `ActiveSessions` to `SessionChat` and register/unregister in chat methods

**Files:**
- Modify: `src/chat/session.rs:28-40` (struct), `~51-80` (constructors), `~126-161` (chat), `~165-205` (chat_coding), `~854-912` (run_agent), `~920-990` (run_agent_with_history)

**Step 1: Add `active_sessions` field to `SessionChat`**

Add to the struct definition:

```rust
pub struct SessionChat {
    db: GhostDb,
    provider: Arc<dyn Provider>,
    tool_manager: ToolManager,
    config: Config,
    prompt_renderer: PromptRenderer,
    max_tool_iterations: usize,
    agent_runner: Option<Arc<crate::agents::AgentRunner>>,
    compaction_override: Option<config::CompactionConfig>,
    event_tx: Option<crate::events::SessionEventSender>,
    cwd_override: Option<std::path::PathBuf>,
    channel_id: Option<String>,
    active_sessions: super::interrupt::ActiveSessions,
}
```

**Step 2: Initialize in constructors**

In `from_config()` and `new()`, initialize with a default empty map:

```rust
use super::interrupt::ActiveSessions;

// In new():
active_sessions: Arc::new(dashmap::DashMap::new()),
```

Add a builder method:

```rust
#[must_use]
pub fn with_active_sessions(mut self, active_sessions: ActiveSessions) -> Self {
    self.active_sessions = active_sessions;
    self
}
```

Add a public accessor for the Discord handler:

```rust
pub fn active_sessions(&self) -> &ActiveSessions {
    &self.active_sessions
}
```

**Step 3: Register/unregister in `chat()`**

In `SessionChat::chat()`, create an interrupt channel, register before the tool loop, unregister after:

```rust
pub async fn chat(
    &self,
    session_id: &str,
    user_message: &str,
    event_tx: Option<&EventSender>,
) -> Result<(ChatResult, RunMetadata), ChatError> {
    let session_thing = parse_session_thing(session_id)?;
    db::sessions::get_session(&self.db, &session_thing).await?;
    db::sessions::update_activity(&self.db, &session_thing).await?;
    db::sessions::create_message(&self.db, &session_thing, "user", user_message).await?;

    let (mut history, stored_ids) = self.load_provider_history(&session_thing).await?;
    self.compact_if_needed(&session_thing, &mut history, &stored_ids)
        .await;

    let model = self.default_model_name()?;
    let effort = resolve_reasoning_effort(None, None, self.model_reasoning_effort());
    let mut handler = ChatHandler {
        session_chat: self,
        session_thing: &session_thing,
        event_tx,
        pending_todo_update: false,
    };

    let (int_tx, int_rx) = super::interrupt::channel();
    self.active_sessions.insert(session_id.to_string(), int_tx);

    let result = run_tool_loop(
        self,
        session_id,
        &model,
        self.max_tool_iterations,
        effort,
        &mut handler,
        &mut history,
        event_tx,
        Some(int_rx),
    )
    .await;

    self.active_sessions.remove(session_id);
    result
}
```

**Step 4: Same pattern for `chat_coding()`**

Same register/unregister pattern. Create `(int_tx, int_rx)`, insert before `run_tool_loop`, remove after.

**Step 5: Same pattern for `run_agent()` and `run_agent_with_history()`**

Same register/unregister pattern for both agent methods.

**Step 6: Verify it compiles and tests pass**

Run: `just ci`
Expected: all green

**Step 7: Commit**

```
git add src/chat/session.rs
git commit -m "feat: register/unregister active sessions around tool loop"
```

---

### Task 5: Wire `ActiveSessions` into Discord handler and add steering + `/stop`

**Files:**
- Modify: `src/interfaces/discord/bot.rs:67-91` (Handler struct + new), `~211-420` (handle_message), `~427-509` (handle_coding_message)
- Modify: `src/interfaces/discord/start.rs:81-135` (start_discord)
- Modify: `src/daemon/run.rs:100-125` (boot)

**Step 1: Add `ActiveSessions` to `Handler`**

In `src/interfaces/discord/bot.rs`, add the field and update `new()`:

```rust
use crate::chat::ActiveSessions;

pub(super) struct Handler {
    session_chat: Arc<SessionChat>,
    db: GhostDb,
    config: Config,
    allowed_user_ids: Vec<String>,
    bot_user_id: OnceLock<String>,
    started_at: std::time::SystemTime,
    active_sessions: ActiveSessions,
}

impl Handler {
    pub fn new(
        session_chat: Arc<SessionChat>,
        db: GhostDb,
        config: Config,
        allowed_user_ids: Vec<String>,
        active_sessions: ActiveSessions,
    ) -> Self {
        Self {
            session_chat,
            db,
            config,
            allowed_user_ids,
            bot_user_id: OnceLock::new(),
            started_at: std::time::SystemTime::now(),
            active_sessions,
        }
    }
```

**Step 2: Add `/stop` command handling in `handle_message()`**

After the existing `/kill` command block (around line 309) and before the coding session check, add `/stop`:

```rust
// Handle /stop command — gracefully stop a running tool loop
if content.eq_ignore_ascii_case("/stop") {
    let session_id = match self.resolve_session(msg.channel_id).await {
        Ok(id) => id,
        Err(e) => {
            error!("Failed to resolve session for /stop: {e}");
            return;
        }
    };

    if let Some(tx) = self.active_sessions.get(&session_id) {
        let _ = tx.send(crate::chat::interrupt::Interrupt::Stop);
        let _ = send_gateway_v2(
            &ctx.http,
            msg.channel_id,
            "Stopping after current operation finishes.",
            None,
        )
        .await;
    } else {
        let _ = send_gateway_v2(
            &ctx.http,
            msg.channel_id,
            "Nothing is running right now.",
            Some(WARNING_EMBED_COLOR),
        )
        .await;
    }
    return;
}
```

**Step 3: Add steering routing in `handle_message()`**

After resolving the session (around line 363, after `let session_id = ...`) and before the typing indicator, add:

```rust
// If a tool loop is already running for this session, steer it
if let Some(tx) = self.active_sessions.get(&session_id) {
    let _ = tx.send(crate::chat::interrupt::Interrupt::Steer {
        message: full_content,
    });
    return;
}
```

**Step 4: Add steering routing in `handle_coding_message()`**

Similarly, at the start of `handle_coding_message()`, after building `full_content` and the empty check, add:

```rust
// If a tool loop is already running for this coding session, steer it
if let Some(tx) = self.active_sessions.get(session_id) {
    let _ = tx.send(crate::chat::interrupt::Interrupt::Steer {
        message: full_content,
    });
    return;
}
```

**Step 5: Update `start_discord()` to pass `ActiveSessions`**

In `src/interfaces/discord/start.rs`, update the function signature and handler creation:

```rust
pub async fn start_discord(
    config: &Config,
    session_chat: Arc<SessionChat>,
    db: GhostDb,
    active_sessions: ActiveSessions,
) -> Result<Option<(DiscordSender, JoinHandle<()>)>, DiscordError> {
```

Add the import:
```rust
use crate::chat::ActiveSessions;
```

Update the handler construction:
```rust
let handler = super::bot::Handler::new(
    session_chat,
    db,
    config.clone(),
    config.discord.allowed_user_ids.clone(),
    active_sessions,
);
```

**Step 6: Update `boot()` in `src/daemon/run.rs`**

Create the shared `ActiveSessions` and pass it through:

```rust
use crate::chat::ActiveSessions;

// After creating session_chat, before start_discord:
let active_sessions: ActiveSessions = Arc::new(dashmap::DashMap::new());

let session_chat = Arc::new(
    SessionChat::from_config(db.clone(), config.clone())?
        .with_agent_runner(Arc::clone(&agent_runner))
        .with_event_sender(event_tx)
        .with_active_sessions(active_sessions.clone()),
);

let discord_result = discord::start_discord(
    &config,
    session_chat.clone(),
    db.clone(),
    active_sessions,
).await?;
```

**Step 7: Verify it compiles and tests pass**

Run: `just ci`
Expected: all green

**Step 8: Commit**

```
git add src/interfaces/discord/bot.rs src/interfaces/discord/start.rs src/daemon/run.rs
git commit -m "feat: wire interrupts into Discord handler with /stop and steering"
```

---

### Task 6: Handle `ChatStopReason::Stopped` in Discord response

**Files:**
- Modify: `src/interfaces/discord/bot.rs` (inside `handle_message` and `handle_coding_message` match arms)

**Step 1: Add Stopped feedback in `handle_message()`**

In the `Ok((result, metadata))` arm of `handle_message()`, after the existing `MaxIterations` warning, add:

```rust
if result.stop_reason == ChatStopReason::Stopped {
    let _ = send_gateway_v2(
        &ctx.http,
        msg.channel_id,
        "Stopped.",
        None,
    )
    .await;
}
```

The last assistant message (if any) is still sent via `send_assistant_v2` as usual — this just adds a confirmation.

**Step 2: Same in `handle_coding_message()`**

Same pattern in the coding handler's match arm.

**Step 3: Verify it compiles and tests pass**

Run: `just ci`
Expected: all green

**Step 4: Commit**

```
git add src/interfaces/discord/bot.rs
git commit -m "feat: show 'Stopped' feedback in Discord on graceful stop"
```

---

### Task 7: Handle `Stopped` in event handler (background agents)

**Files:**
- Modify: `src/daemon/event_handler.rs` — check if `Stopped` needs any special handling

**Step 1: Read `src/daemon/event_handler.rs` and check how chat results are handled**

The event handler calls `session_chat.chat()` for continuation turns. If that returns `Stopped`, it should just log it and not send a continuation — the OPERATOR already knows.

**Step 2: Add Stopped handling if needed**

If the event handler's chat result handling has a match on `stop_reason`, add `Stopped` to the appropriate arm. If it doesn't match on stop_reason, no change needed.

**Step 3: Verify it compiles and tests pass**

Run: `just ci`
Expected: all green

**Step 4: Commit (if changes were made)**

```
git commit -m "fix: handle Stopped reason in event handler"
```

---

### Task 8: Integration test

**Files:**
- Modify: existing test file or create a unit test in `src/chat/interrupt.rs`

Read the `/testing` skill before writing tests.

**Step 1: Unit test the interrupt channel and drain logic**

Add a `#[cfg(test)]` module to `src/chat/interrupt.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steer_and_stop_send_correctly() {
        let (tx, mut rx) = channel();
        tx.send(Interrupt::Steer {
            message: "change direction".into(),
        })
        .unwrap();
        tx.send(Interrupt::Stop).unwrap();

        match rx.try_recv().unwrap() {
            Interrupt::Steer { message } => assert_eq!(message, "change direction"),
            _ => panic!("expected Steer"),
        }
        match rx.try_recv().unwrap() {
            Interrupt::Stop => {}
            _ => panic!("expected Stop"),
        }
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn sender_dropped_returns_err() {
        let (tx, _rx) = channel();
        drop(_rx);
        assert!(tx.send(Interrupt::Stop).is_err());
    }
}
```

**Step 2: Run tests**

Run: `cargo test -p ghost interrupt`
Expected: all pass

**Step 3: Commit**

```
git add src/chat/interrupt.rs
git commit -m "test: unit tests for interrupt channel"
```

---

### Task 9: Final verification and formatting

**Step 1: Run full CI**

Run: `just ci`
Expected: all green — format, check, clippy, tests

**Step 2: Review all changes**

Run: `git diff main --stat` and skim each file for correctness.

**Step 3: Final commit if any formatting fixes**

```
git commit -m "chore: formatting fixes"
```
