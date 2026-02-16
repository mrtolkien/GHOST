# 06 — Chat Orchestration and Session Management

## Overview

The chat orchestration layer manages the conversation loop between the OPERATOR and the
GHOST. It handles message history, tool execution loops, and session persistence.

This is the heart of the system — the equivalent of t-koma's `SessionChat`.

## Architecture

```
OPERATOR message (from Discord or CLI)
    ↓
SessionChat::chat()
    ↓
Build message history (load from DB + compaction summary)
    ↓
Render system prompt
    ↓
Send to Provider
    ↓
Process response:
  - If text → return to OPERATOR
  - If tool_use → execute tools → append results → loop back to Provider
    ↓
Persist messages to DB
```

## SessionChat

```rust
pub struct SessionChat {
    db: Surreal<Db>,
    provider: Arc<dyn Provider>,
    tool_manager: ToolManager,
    config: Config,
}

impl SessionChat {
    /// Interactive chat — OPERATOR sends a message, GHOST responds.
    /// Handles the full tool-use loop.
    #[tracing::instrument(skip_all, fields(session_id = %session_id))]
    pub async fn chat(
        &self,
        session_id: &str,
        user_message: &str,
    ) -> Result<ChatResponse>;

    /// Job chat — same loop but with a job prompt instead of user message.
    /// Transcript goes to job_log, not session messages.
    #[tracing::instrument(skip_all, fields(job_name = %job_name))]
    pub async fn chat_job(
        &self,
        job_name: &str,
        session_id: &str,
        prompt: &str,
        tool_set: ToolSet,
    ) -> Result<JobTranscript>;
}
```

## Message History

Provider-neutral message types stored in DB:

```rust
pub struct StoredMessage {
    pub id: Thing,
    pub session: Thing,
    pub role: Role,
    pub content: String,          // Plain text content
    pub tool_calls: Option<Vec<ToolCall>>,    // Assistant's tool calls
    pub tool_results: Option<Vec<ToolResult>>, // Tool execution results
    pub created_at: DateTime<Utc>,
}
```

### History Assembly

1. Load compaction summary (if any) as a system message
2. Load messages after compaction cursor
3. Append the new user message
4. Prepend the rendered system prompt

## Tool Execution Loop

When the provider returns `StopReason::ToolUse`:

1. Extract tool calls from the response
2. Execute each tool via `ToolManager`
3. Collect results
4. Append the assistant message (with tool calls) and tool results to history
5. Send the updated history back to the provider
6. Repeat until `StopReason::EndTurn` or `StopReason::MaxTokens`

Safety: cap tool loops at a configurable maximum (default: 25 iterations) to prevent
infinite loops. When the cap is hit, return `StopReason::MaxIterations` so the caller
can decide how to handle it (e.g., ask the OPERATOR to continue — see spec 09).

## Session Lifecycle

- Sessions are created implicitly on first message from a new conversation context.
- Each interface (e.g., Discord channel) has one **active** session at a time, tracked
  via the `interface_session` table (see spec 09). A channel can have multiple sessions
  over its lifetime via `/REBOOT`.
- `last_activity_at` is updated on every OPERATOR message
- Sessions are never deleted — only the compaction system manages history length
- A session can have multiple job logs attached to it

### Session Reboot

The OPERATOR can reboot a session via `/REBOOT` (see spec 09). Rebooting means:

1. Mark the current session as `rebooted` (preserves history for reference)
2. Create a new session for the same interface/channel
3. The new session starts fresh — clean system prompt, no chat history

```rust
impl SessionChat {
    /// Reboot a session: mark old one as rebooted, create a new one.
    /// Returns the new session ID.
    /// The pre-reboot reflection trigger is wired in spec 17.
    #[tracing::instrument(skip_all, fields(old_session_id = %session_id))]
    pub async fn reboot_session(&self, session_id: &str) -> Result<String>;
}
```

At this step, `reboot_session()` just handles the session swap. The reflection-before-
reboot behavior is added when the reflection subsystem exists (spec 17).

## Differences Between `chat()` and `chat_job()`

| Aspect           | `chat()`               | `chat_job()`              |
| ---------------- | ---------------------- | ------------------------- |
| Trigger          | OPERATOR message       | Scheduler / manual run    |
| Messages stored  | In `message` table     | In `job_log.transcript`   |
| Tool set         | Chat tools             | Job-specific tools        |
| Response goes to | OPERATOR (via Discord) | Job log + optional notify |
| System prompt    | Full system prompt     | Job-specific prompt       |

## Validation

1. `cargo test` — send a message via `SessionChat::chat()` with a mock provider, verify
   a response is returned
2. `cargo test` — tool loop: mock provider returns `StopReason::ToolUse`, verify the
   loop executes the tool and sends results back
3. `cargo test` — max iterations: mock provider always returns tool_use, verify the loop
   stops at the cap and returns `StopReason::MaxIterations`
4. `cargo test` — messages are persisted: after a chat round-trip, query SurrealDB and
   verify both user and assistant messages exist
5. `cargo test` — `reboot_session()` marks the old session and creates a new one with a
   different ID
6. `just ci` — passes

## Acceptance Criteria

- `SessionChat::chat()` accepts a message and returns a provider response
- Tool use loop executes tools and sends results back to the provider
- Tool loop is capped at max iterations
- Messages are persisted to SurrealDB after each exchange
- Session `last_activity_at` is updated on OPERATOR messages
- `chat_job()` stores transcript in job_log, not session messages
- All operations produce tracing spans
- Errors in tool execution don't crash the chat loop — they're returned as tool errors
- `reboot_session()` marks old session and creates a new one
- `just ci` passes

## Prior Art

Old code in `../t-koma`:

- `t-koma-gateway/src/session.rs` — `SessionChat` with `chat()` and `chat_job()`. Core
  logic (tool loop, message persistence, job transcript separation) is directly
  reusable. The data layer changes (SurrealDB vs SQLite) but the orchestration pattern
  is the same.
- `t-koma-gateway/src/chat/history.rs` — Provider-neutral chat history types. Reusable
  type definitions.
