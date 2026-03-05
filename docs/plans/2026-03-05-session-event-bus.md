# Session Event Bus — Design

## Status: DRAFT

## Problem

Background tasks (shell commands, agents) need to push results back to the session that
spawned them and trigger a continuation chat turn. Today this is done by two separate
mechanisms:

1. **Completion watcher** (`src/daemon/completion_watcher.rs`): Receives
   `CompletionEvent::ShellCommand` via an mpsc channel when a background shell command
   finishes. Waits for session idle, injects a system message, triggers a chat turn, and
   sends the response to Discord.

2. **Agent watcher** (`src/agents/watcher.rs`): Polls `AgentRunner.handles` every 3
   seconds checking `JoinHandle.is_finished()`. When an agent completes, injects
   findings as a system message, triggers a chat turn, and sends the response to
   Discord.

Both do the same thing — deliver a system message to a session and trigger continuation
— but use different mechanisms (channel vs polling) and live in different modules.
Neither handles coding agent sessions: both call `session_chat.chat()` (GHOST handler)
and resolve Discord channels via `interface_sessions` only.

## Goal

Unify background-task-to-session delivery into a single event bus. One channel, one
event type, one consumer. Fix coding session support as a natural consequence.

## Design

### Event Type

```rust
// src/events.rs (replaces src/completion.rs)

/// An event requesting delivery of a system message to a session,
/// followed by a continuation chat turn.
pub struct SessionEvent {
    /// Target session ID
    pub session_id: String,
    /// System message to inject before triggering continuation
    pub system_message: String,
    /// Optional metadata for Discord presentation
    pub discord: Option<DiscordPayload>,
}

pub struct DiscordPayload {
    /// Pre-formatted agent summary for Discord embed
    pub agent_summary: Option<String>,
}

pub type SessionEventSender = mpsc::UnboundedSender<SessionEvent>;
pub type SessionEventReceiver = mpsc::UnboundedReceiver<SessionEvent>;

pub fn channel() -> (SessionEventSender, SessionEventReceiver) {
    mpsc::unbounded_channel()
}
```

The event is about the **session**, not the task. Producers format their own system
message content. The consumer doesn't need to know what completed — it just delivers.

### Producers

**Shell background task** (`src/tools/shell.rs`): Currently sends
`CompletionEvent::ShellCommand`. Change to send `SessionEvent` with message content
`[shell-command completed]\n$ {cmd}\n\n{output}`.

**Agent runner** (`src/agents/runner.rs`): `finish_background` currently writes to DB
and stores metadata in the handle. Add: send `SessionEvent` with message content
`[agent:{name} completed]\n\n{findings}` and `DiscordPayload` containing the agent
summary embed.

Both producers already have access to everything they need to format the message. The
shell task has the command and output. `finish_background` has the agent name, findings,
and metadata.

### Consumer

One task: `src/daemon/event_handler.rs` (replaces both `completion_watcher.rs` and
`agents/watcher.rs`).

```
loop {
    receive SessionEvent
    wait_for_idle(session_id)
    resolve session type:
        if coding_sessions has active session for session_id:
            build coding prompt
            session_chat.chat_coding(session_id, trigger, prompt)
        else:
            session_chat.chat(session_id, trigger)
    resolve Discord channel:
        check interface_sessions(session_id)
        fallback: check coding_sessions(session_id).channel_id
    send response to Discord
    if discord_payload.agent_summary:
        send agent summary embed
}
```

### Plumbing

**`AgentRunner`**: Add `event_tx: Option<SessionEventSender>` field. Passed at
construction. Flows into `BackgroundTask`. Used by `finish_background` to send the
event.

**`ToolContext`**: Replace `completion_tx: Option<CompletionSender>` with
`event_tx: Option<SessionEventSender>`. Shell tool uses it to send shell completion
events.

**`SessionChat`**: Replace `completion_tx` / `with_completion_sender` with `event_tx` /
`with_event_sender`.

**`daemon/run.rs`**: Create one channel. Clone the sender into both `AgentRunner` and
`SessionChat`. Spawn one `event_handler` task. Remove `spawn_agent_watcher` and
`spawn_completion_watcher`.

### New DB Query

`db::coding_sessions::get_active_coding_session_by_chat_session(db, session_id)` returns
`Option<(working_dir, channel_id)>`.

Used by the event handler to detect coding sessions and resolve their Discord channel.

### Handle Cleanup

`AgentRunner.handles` stays for `agent_control` status/stop support. But
`take_completed` is removed — the polling watcher was its only caller.

Handle cleanup moves to `finish_background`: after sending the `SessionEvent`, remove
the handle from the map. This requires `finish_background` to have a reference to the
handles map (add `handles: Arc<Mutex<HashMap<...>>>` to `BackgroundTask`).

### What Gets Deleted

| File                               | Reason                                          |
| ---------------------------------- | ----------------------------------------------- |
| `src/completion.rs`                | Replaced by `src/events.rs`                     |
| `src/daemon/completion_watcher.rs` | Replaced by `src/daemon/event_handler.rs`       |
| `src/agents/watcher.rs`            | Replaced by event sent from `finish_background` |

### What Stays Unchanged

| Component                          | Why                                                     |
| ---------------------------------- | ------------------------------------------------------- |
| Tool loop events (`ToolLoopEvent`) | Per-turn streaming UI, different lifecycle              |
| File watcher (`daemon/watcher.rs`) | Watches filesystem, doesn't deliver to sessions         |
| Scheduler (`agents/scheduler.rs`)  | Triggers agent runs; completion events come from runner |
| `AgentRunner` status/stop/list     | Still needed for `agent_control` tool                   |

## Key Design Decisions

1. **mpsc, not broadcast**: One consumer handles all delivery. No event type needs
   multiple independent subscribers. Standard `tokio::sync::mpsc` is sufficient.

2. **Event is session-scoped, not task-scoped**: The consumer doesn't need to know
   whether a shell command or an agent completed. It just delivers a system message and
   triggers continuation. Producers format their own content.

3. **Session type resolution in consumer**: The consumer checks `coding_sessions` to
   decide whether to call `chat_coding` or `chat`. This is the single place where
   GHOST-vs-coding routing lives.

4. **Handles cleaned up by producer**: `finish_background` removes its own handle after
   sending the event, rather than relying on a poller to call `take_completed`.

5. **Tool loop events excluded**: They have a fundamentally different lifecycle
   (per-turn, real-time streaming, scoped to one chat turn) and are already well-served
   by their per-turn channel pattern.
