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
infinite loops.

## Session Lifecycle

- Sessions are created implicitly on first message from a new conversation context
- `last_activity_at` is updated on every OPERATOR message
- Sessions are never deleted — only the compaction system manages history length
- A session can have multiple job logs attached to it

## Differences Between `chat()` and `chat_job()`

| Aspect           | `chat()`               | `chat_job()`              |
| ---------------- | ---------------------- | ------------------------- |
| Trigger          | OPERATOR message       | Scheduler / manual run    |
| Messages stored  | In `message` table     | In `job_log.transcript`   |
| Tool set         | Chat tools             | Job-specific tools        |
| Response goes to | OPERATOR (via Discord) | Job log + optional notify |
| System prompt    | Full system prompt     | Job-specific prompt       |

## Acceptance Criteria

- OPERATOR can send a message and receive a response
- Tool use loop executes tools and sends results back to the provider
- Tool loop is capped at max iterations
- Messages are persisted to SurrealDB after each exchange
- Session `last_activity_at` is updated on OPERATOR messages
- `chat_job()` stores transcript in job_log, not session messages
- All operations produce tracing spans
- Errors in tool execution don't crash the chat loop — they're returned as tool errors

## Prior Art

Old code in `../t-koma`:

- `t-koma-gateway/src/session.rs` — `SessionChat` with `chat()` and `chat_job()`. Core
  logic (tool loop, message persistence, job transcript separation) is directly
  reusable. The data layer changes (SurrealDB vs SQLite) but the orchestration pattern
  is the same.
- `t-koma-gateway/src/chat/history.rs` — Provider-neutral chat history types. Reusable
  type definitions.
