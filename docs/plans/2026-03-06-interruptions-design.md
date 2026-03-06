# Interruptions & Steering — Design

> Let the OPERATOR send messages to the GHOST (or coding agent) during an agentic tool
> loop, and stop a running loop gracefully.

## Prior Art

Researched: Claude Code, Aider, Cursor, GitHub Copilot, Pi Mono, Devin, Cline, Windsurf.

Best implementations (Pi Mono, Cursor, Copilot) share a pattern:

- Steering messages are delivered **between tool calls** as regular user messages.
- The current tool finishes; the message is injected before the next provider call.
- The model sees its own tool calls, results, then the user's message, and adapts.
- Cancel/stop is a separate mechanism from steering.

## Decisions

| Question        | Decision                                                                                                                                                        |
| --------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Message types   | Single type: "interrupt". No steer vs follow-up distinction.                                                                                                    |
| Delivery timing | Between tool calls. Current tool finishes, message injected before next provider call.                                                                          |
| Stop mechanism  | `/stop` Discord command. Graceful: current tool finishes, loop exits. No extra provider call.                                                                   |
| Message role    | `Role::User` — regular user message, same as any OPERATOR message.                                                                                              |
| DB persistence  | Yes. Steering messages are persisted like all other messages.                                                                                                   |
| Routing         | Automatic. If a tool loop is running for the session, the message routes as steering. If idle, starts a new `chat()` call. No special syntax from the OPERATOR. |
| Architecture    | `tokio::mpsc` channel into the tool loop. Shared `ActiveSessions` registry.                                                                                     |

## Architecture

### New Module: `src/chat/interrupt.rs`

```rust
pub enum Interrupt {
    /// Inject a user message between tool calls.
    Steer { message: String },
    /// Stop the tool loop gracefully after the current tool finishes.
    Stop,
}

pub type InterruptSender = tokio::sync::mpsc::UnboundedSender<Interrupt>;
pub type InterruptReceiver = tokio::sync::mpsc::UnboundedReceiver<Interrupt>;

pub fn channel() -> (InterruptSender, InterruptReceiver) {
    tokio::sync::mpsc::unbounded_channel()
}

/// Tracks which sessions have a running tool loop.
/// Key: session_id, Value: sender to interrupt that loop.
pub type ActiveSessions = Arc<DashMap<String, InterruptSender>>;
```

### Tool Loop Changes (`src/chat/tool_loop.rs`)

`run_tool_loop()` gains an `Option<&mut InterruptReceiver>` parameter.

**Check point**: At the top of the main loop, after `post_tool_iteration()` returns and
before building the next `ChatRequest`, drain all pending interrupts:

```
loop {
    // ... (existing) post_tool_iteration, pre-send compaction ...

    // NEW: check for interrupts
    match drain_interrupts(interrupt_rx, history, handler, session_id) {
        InterruptAction::Continue => {}
        InterruptAction::Stop => {
            // return last_result with ChatStopReason::Stopped
        }
    }

    // ... (existing) build ChatRequest, send to provider ...
}
```

**Drain logic**:

```rust
fn drain_interrupts(...) -> InterruptAction {
    while let Ok(interrupt) = rx.try_recv() {
        match interrupt {
            Interrupt::Stop => return InterruptAction::Stop,
            Interrupt::Steer { message } => {
                // Persist to DB
                db::sessions::create_message(db, session_id, "user", &message).await;
                // Append to in-memory history
                history.push(ChatMessage {
                    role: Role::User,
                    content: vec![ContentBlock::Text { text: message }],
                });
            }
        }
    }
    InterruptAction::Continue
}
```

If multiple steers are queued, they all get appended (in order) before the next provider
call. If a Stop is among them, the steers before it are still appended (they're
persisted as part of the conversation record) but the loop exits.

### New Stop Reason

Add `ChatStopReason::Stopped` to `src/chat/types.rs`. This signals to callers that the
loop was interrupted by the OPERATOR, not by the model ending its turn or hitting
limits.

### Session Registration (`src/chat/session.rs`)

`SessionChat` gets an `active_sessions: ActiveSessions` field, set via builder.

In `chat()` and `chat_coding()`:

```rust
pub async fn chat(&self, session_id: &str, ...) -> Result<...> {
    let (tx, mut rx) = interrupt::channel();
    // Register — other callers can now send interrupts
    self.active_sessions.insert(session_id.to_string(), tx);

    let result = run_tool_loop(..., Some(&mut rx)).await;

    // Unregister — session is idle again
    self.active_sessions.remove(session_id);

    result
}
```

`run_agent()` and `run_agent_with_history()` also register, so background agents can be
steered/stopped too.

### Discord Handler Changes (`src/interfaces/discord/bot.rs`)

The Discord `Handler` gets access to `ActiveSessions`.

**Message routing** in `handle_message()`:

```rust
if let Some(tx) = self.active_sessions.get(&session_id) {
    // Session has a running tool loop — steer it
    tx.send(Interrupt::Steer { message: content }).ok();
    // Optionally: react with an emoji to confirm receipt
    return;
}
// No active loop — start a new chat() call as before
```

**`/stop` command**:

Register a Discord slash command `/stop`. Handler:

```rust
if let Some(tx) = self.active_sessions.get(&session_id) {
    tx.send(Interrupt::Stop).ok();
    // Reply: "Stopping after current tool call finishes."
}
```

If no loop is running, reply: "Nothing is running right now."

### Daemon Wiring (`src/daemon/run.rs`)

Create `ActiveSessions` at boot, pass to both `SessionChat` and the Discord `Handler`:

```rust
let active_sessions: ActiveSessions = Arc::new(DashMap::new());

let session_chat = SessionChat::from_config(db, config)
    .with_active_sessions(active_sessions.clone());

// Pass to Discord handler
let handler = Handler::new(..., active_sessions.clone());
```

## Message Flow Diagrams

### Steering

```
OPERATOR (Discord)          Discord Handler            Tool Loop
    |                           |                         |
    |-- "use pytest not cargo" ->|                         |
    |                           |-- active_sessions.get() |
    |                           |   found! ------------->|
    |                           |-- tx.send(Steer{...})  |
    |                           |                         |
    |                           |   (current tool finishes)
    |                           |                         |-- try_recv()
    |                           |                         |-- got Steer
    |                           |                         |-- persist to DB
    |                           |                         |-- push to history
    |                           |                         |-- next provider call
    |                           |                         |   (model sees message)
```

### Stop

```
OPERATOR (Discord)          Discord Handler            Tool Loop
    |                           |                         |
    |-- /stop ----------------->|                         |
    |                           |-- tx.send(Stop) ------>|
    |                           |                         |
    |                           |   (current tool finishes)
    |                           |                         |-- try_recv()
    |                           |                         |-- got Stop
    |                           |                         |-- return Stopped
    |  <-- "Stopped." ---------|<-- result ---------------|
```

## Edge Cases

**Multiple steers before next check**: All are drained and appended in order. The model
sees them all on its next turn.

**Steer arrives during provider API call (not tool execution)**: The `try_recv()` check
happens at the top of the loop, before the provider call. If a steer arrives during the
provider call, it won't be seen until after that call returns and the resulting tool
calls execute. This is acceptable — the message will be picked up on the next iteration.

**Steer arrives as model returns EndTurn**: The tool loop unregisters from
`ActiveSessions` and the `InterruptSender` is dropped. The Discord handler's `tx.send()`
returns `Err`, it falls through to starting a new `chat()` call with that message —
normal idle behavior. No special handling needed.

**`/stop` when no loop is running**: Discord handler replies "Nothing running."

**Tool loop finishes naturally while steer is in-flight**: The `active_sessions` entry
is removed. `tx.send()` returns `Err` (receiver dropped). Discord handler falls through
to starting a new `chat()` call with the message.

**Race between unregister and new message**: Use `DashMap::remove()` which is atomic. If
the Discord handler calls `get()` and finds the entry, the tool loop hasn't finished
yet, so the channel is still valid.

**Coding agent sessions**: Same mechanism. `chat_coding()` registers in
`active_sessions` the same way. The coding agent's tool loop checks for interrupts
identically.

**Background Lua agents**: Also register. The OPERATOR can steer or stop background
agents. The session ID is the routing key — Discord channel maps to session, and the
interrupt finds the right loop.

## What This Does NOT Cover

- **Interrupting mid-provider-call** (cancelling a streaming response): Out of scope.
  Would require provider-level cancellation support.
- **Multiple concurrent tool loops per session**: Not possible today, not needed.
- **UI feedback in Discord** (typing indicator, "message received" reaction):
  Nice-to-have, not in scope for the core mechanism.
- **Follow-up messages** (queued for after the loop finishes): Decided against. Single
  message type only.

## Files to Modify

| File                            | Change                                                                                       |
| ------------------------------- | -------------------------------------------------------------------------------------------- |
| `src/chat/interrupt.rs`         | New module — `Interrupt` enum, channel, `ActiveSessions` type                                |
| `src/chat/mod.rs`               | Add `pub mod interrupt;`                                                                     |
| `src/chat/tool_loop.rs`         | Add `interrupt_rx` param, drain logic, `Stopped` handling                                    |
| `src/chat/types.rs`             | Add `ChatStopReason::Stopped`                                                                |
| `src/chat/session.rs`           | Add `active_sessions` field, register/unregister in `chat()`, `chat_coding()`, `run_agent()` |
| `src/interfaces/discord/bot.rs` | Route messages to interrupt channel, add `/stop` command                                     |
| `src/daemon/run.rs`             | Create `ActiveSessions`, wire to `SessionChat` and Discord handler                           |
