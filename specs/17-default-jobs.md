# 17 — Heartbeat and Reflection Subsystems

## Overview

Heartbeat and reflection are **dedicated subsystems** in the daemon, not regular jobs.
They have their own code paths, timing configuration, and execution logic.

This matches the t-koma approach. The key difference from the original plan: event-based
triggers and the unified job system are deferred to post-PoC (see
`backlog/lua-jobs.md`).

> **Future**: When the Lua jobs system is built, heartbeat and reflection will be
> migrated to event-triggered Lua jobs. The dedicated code paths built here will be
> replaced by:
>
> - Heartbeat: a Lua job with `trigger = { event = "session_idle", delay = "4m" }`
> - Reflection: a Lua job with
>   `trigger = { event = "job_completed", job = "heartbeat" }`
>
> The prompts and logic documented here will inform the Lua job implementations.

## Heartbeat

### Purpose

Check on the OPERATOR during idle periods. The GHOST can proactively share relevant
information, ask follow-up questions, or simply check in.

### Trigger

The daemon monitors all active sessions for idle time. When no OPERATOR message has been
received for `timing.heartbeat_idle_minutes` (default: 4 minutes), the heartbeat fires
for that session.

```
OPERATOR sends message → timer starts
    ↓ (4 minutes, no new messages)
Heartbeat fires for that session
    ↓
Cooldown starts (30 minutes)
```

### Skip Logic

- Skip if no messages at all in the session since last heartbeat (`skip_if_no_activity`)
- Skip if cooldown hasn't elapsed (`timing.heartbeat_continue_minutes`)
- The daemon checks for idle sessions every `timing.heartbeat_check_seconds` (default:
  60s)

### Heartbeat Prompt

The heartbeat prompt is embedded as a default but can be overridden by placing
`$WORKSPACE/heartbeat.md` in the workspace:

```markdown
# Heartbeat Check

You are running a heartbeat check. The OPERATOR has been idle for a few minutes.

Review the recent conversation and decide:

1. Is there something useful you can proactively share?
2. Is there a follow-up question worth asking?
3. Is there a task you can work on in the background?

If you have something genuinely useful, send a brief message to the OPERATOR.

If there's nothing meaningful to say, respond with exactly: HEARTBEAT_CONTINUE

This will suppress any output and reschedule the next heartbeat.
```

### Heartbeat Execution

1. Load the heartbeat prompt (workspace override or embedded default)
2. Run via `SessionChat::chat_job()` **in the idle session's context** (with full
   message history)
3. Check the response for `HEARTBEAT_CONTINUE`:
   - If found → suppress output, log as "skipped", reset cooldown timer
   - Otherwise → send the response to the OPERATOR's Discord channel
4. Save transcript to `job_log` table
5. Save response to `.state/heartbeat.last.md` for continuity

### Sending to Discord

The heartbeat subsystem needs a reference to the Discord sender to push unsolicited
messages. The `interface_session` table (see spec 09) maps the session back to its
Discord channel, so the heartbeat can send to the correct channel.

```rust
// src/jobs/heartbeat.rs

pub struct HeartbeatManager {
    db: Surreal<Db>,
    session_chat: Arc<SessionChat>,
    discord_sender: Arc<DiscordSender>,
    config: TimingConfig,
}
```

## Reflection

Lives in `src/jobs/reflection.rs`.

### Purpose

Autonomous knowledge curation. After conversations, the GHOST reviews what was discussed
and organizes information into notes, references, and diary entries.

### Triggers

Reflection fires in two situations:

**After heartbeat:**

```
Heartbeat completes (status = "ok")
    ↓
Check: has there been new activity since last reflection?
    ↓ (yes)
Reflection fires, cooldown = timing.reflection_idle_minutes (default: 4m after heartbeat)
```

**On session reboot:**

```
OPERATOR sends /REBOOT
    ↓
Reflection runs immediately on the old session (no cooldown check)
    ↓
Session is rebooted (new session created)
```

This ensures no knowledge is lost when the OPERATOR reboots. The reflection runs against
the old session's history before it becomes inactive. Wire this into
`SessionChat::reboot_session()` (spec 06) by calling `ReflectionManager::run()` before
the session swap.

### Skip Logic

- Skip if no new session activity since the last reflection run (heartbeat trigger only
  — reboot always runs)
- Only one reflection runs at a time (mutex)

### Reflection Prompt

The reflection prompt is embedded as a default but can be overridden by placing
`$WORKSPACE/reflection.md` in the workspace:

```markdown
# Reflection — Knowledge Curation

You are in autonomous reflection mode. No OPERATOR is present. Review the conversation
transcript below and organize knowledge.

## Note Writing Guidelines

[... adapted from t-koma reflection prompt ...]

### Wiki Links

Use `[[Target]]` for default relationships or `[[relationship>Target]]` for typed edges.

Examples:

- `[[Rust]]` — creates a default `relates_to` edge
- `[[written_in>Rust]]` — creates a `written_in` edge
- `[[depends_on>tokio]]` — creates a `depends_on` edge

## Your Input

### Previous Handoff Note

{{ previous_handoff }}

### Today's Diary

{{ diary_today }}

### Conversation Transcript (filtered)

{{ recent_messages }}

### Cached Web Results

{{ web_cache_files }}

## Workflow

1. **Plan**: Create a TODO list of knowledge operations
2. **Execute**: Create/update notes, curate web cache, write diary, update identity
3. **Handoff**: Your final message becomes the handoff note for the next reflection
```

### Reflection Execution

1. Load the reflection prompt (workspace override or embedded default)
2. Interpolate template variables:
   - `{{ previous_handoff }}` — from `.state/reflection.last.md`
   - `{{ diary_today }}` — today's diary entry from SurrealDB
   - `{{ recent_messages }}` — filtered transcript (see below)
   - `{{ web_cache_files }}` — list of files in `.web-cache/`
3. Run via `SessionChat::chat_job()` in the **same session** as the heartbeat (with full
   message history for context)
4. Use the **reflection tool set** (knowledge write tools)
5. Save transcript to `job_log` table
6. Save response to `.state/reflection.last.md` (handoff note)

### Transcript Filtering

The reflection prompt receives a filtered transcript:

- **Preserved**: User and assistant text messages
- **Summarized**: Tool calls (name + brief summary, not full input)
- **Stripped**: Tool results (too verbose, reflection can use knowledge_search to look
  things up)

### Post-Reflection: Web Cache Clear

After a successful reflection run, the `.web-cache/` directory is cleared. This is
implemented as a post-processing step in the reflection subsystem.

## Config

```toml
[timing]
heartbeat_idle_minutes = 4 # Idle time before heartbeat fires
heartbeat_check_seconds = 60 # How often to check for idle sessions
heartbeat_continue_minutes = 30 # Cooldown between heartbeats
reflection_idle_minutes = 4 # Delay after heartbeat before reflection
```

## Prompt Customization

Both prompts can be overridden by placing files in the workspace:

```
$WORKSPACE/
├── heartbeat.md       # Optional: overrides embedded heartbeat prompt
├── reflection.md      # Optional: overrides embedded reflection prompt
└── ...
```

If the file doesn't exist, the embedded default is used. This lets the OPERATOR
customize behavior without touching code.

## Observability

```rust
#[tracing::instrument(skip_all, fields(session_id = %session_id))]
async fn run_heartbeat(&self, session_id: &str) -> Result<HeartbeatOutcome> {
    logfire::info!("heartbeat started", session_id = %session_id);
    // ...
    logfire::info!("heartbeat completed",
        session_id = %session_id,
        outcome = %outcome, // "sent", "suppressed", "skipped"
    );
}

#[tracing::instrument(skip_all, fields(session_id = %session_id))]
async fn run_reflection(&self, session_id: &str) -> Result<()> {
    logfire::info!("reflection started", session_id = %session_id);
    // ...
    logfire::info!("reflection completed",
        session_id = %session_id,
        notes_written = notes_count,
        web_cache_cleared = cache_cleared,
    );
}
```

## Validation

1. `cargo test` — heartbeat fires after the configured idle period (use a short timeout
   and mock timer)
2. `cargo test` — mock provider returns `HEARTBEAT_CONTINUE`, verify output is
   suppressed and cooldown timer resets
3. `cargo test` — mock provider returns a real message, verify it's sent via
   `DiscordSender` to the correct channel
4. `cargo test` — reflection fires after a successful heartbeat when there's new session
   activity
5. `cargo test` — reflection skips when there's no new activity since the last run
6. `cargo test` — reflection on reboot: call `reboot_session()`, verify reflection runs
   on the old session before the swap
7. `cargo test` — handoff note: run reflection twice, verify the second run receives the
   first run's handoff via `.state/reflection.last.md`
8. `cargo test` — web cache cleared after successful reflection, preserved after failure
9. Manual: run the daemon, send a message, wait for idle — heartbeat appears in Discord
10. `just ci` — passes

## Acceptance Criteria

- Heartbeat fires after OPERATOR idle period per `[timing]` config
- `HEARTBEAT_CONTINUE` response suppresses output
- Heartbeat sends output to the correct Discord channel
- Reflection fires after successful heartbeat
- Reflection runs on session reboot (before session swap)
- Reflection has access to reflection tool set (knowledge write tools)
- Filtered transcript is injected into the reflection prompt
- Handoff note carries over between reflection runs via `.state/reflection.last.md`
- Web cache is cleared after successful reflection
- Both prompts can be overridden via workspace files
- Both subsystems log to `job_log` table
- Both subsystems produce tracing spans
- `just ci` passes

## Prior Art

Old code in `../t-koma`:

- `t-koma-gateway/src/heartbeat.rs` — Heartbeat logic: idle detection, skip-if-recent,
  `HEARTBEAT_CONTINUE` response handling. Directly reusable — this is essentially the
  same architecture.
- `t-koma-gateway/src/reflection.rs` — Reflection logic: transcript filtering (text
  preserved, tool-use summarized, tool-result stripped), handoff note continuity, web
  cache listing and post-run clearing. Directly reusable.
- `prompts/system/reflection-prompt.md` — Reflection prompt text. Directly reusable,
  update wiki link syntax for typed edges (`[[rel>Target]]`).
