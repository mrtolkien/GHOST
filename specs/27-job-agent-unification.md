# Spec 27: Job–Agent Execution Unification

## Problem

Jobs (heartbeat, reflection, cron jobs) and agents (deep-research, future coding agent)
share nearly identical execution mechanics but use separate code paths:

- **AgentHandler**: DB-persisted messages, progress rules, TODO injection, background
  execution, continuation support, custom system prompts from TOML definitions
- **JobHandler**: transcript string accumulation, progress rules (recently added), no
  TODO injection, no continuation, hardcoded `"Job: {name}"` system prompt

Every new feature added to one handler must be manually duplicated in the other.
Reflection already needs progress rules, TODO injection, and a meaningful system prompt
— all things AgentHandler already provides.

## Goal

One execution handler for all autonomous LLM work. Jobs and agents differ only in:

- **Trigger**: how/when they start (cron schedule, event, chat command, daemon boot)
- **Context**: what data they receive (transcript, web cache, query, etc.)

The LLM execution loop itself should be identical.

## Design

### Unified definition format

Agents already use markdown + TOML frontmatter. Jobs use the same format (spec 16).
Extend the agent definition to cover all cases:

```toml
+++
name = "reflection"
description = "Knowledge curation after conversation activity"
tools = ["run_shell_command", "read_file", "write_file", "file_edit",
         "todo", "knowledge_search", "web_search", "web_fetch",
         "note_write", "reference_manage"]
max_iterations = 40
# trigger replaces cron schedule — supports multiple trigger types
trigger = "after_heartbeat"
# delay before execution (optional, replaces reflection_idle_minutes)
delay_seconds = 300

[[progress]]
tool = "note_write"
nudge = "You have created {count} notes so far. Do they capture all important entities from the transcript?"

[[progress]]
tool = "reference_manage"
+++

# Reflection Mode — Knowledge Curator
...prompt body...
```

### Trigger types

Instead of the current split between "jobs" (cron) and "agents" (chat-spawned), use a
trigger taxonomy:

| Trigger                | Current equivalent   | When it fires                |
| ---------------------- | -------------------- | ---------------------------- |
| `cron = "0 9 * * MON"` | Cron job             | On schedule                  |
| `after_heartbeat`      | Reflection           | After heartbeat completes    |
| `on_boot`              | Reflection (reboot)  | Daemon startup               |
| `on_idle`              | Heartbeat            | Session idle for N minutes   |
| `on_demand`            | Agent (chat-spawned) | GHOST or OPERATOR invokes it |

`on_demand` is the default for agents that are spawned by the chat model via
`agent_control`. All other triggers are daemon-managed.

### Single execution handler

Replace `JobHandler` and `AgentHandler` with a single `TaskHandler`:

```rust
struct TaskHandler<'a> {
    session_chat: &'a SessionChat,
    session_thing: &'a Thing,
    system_prompt: String,
    task_name: String,
    progress_rules: Vec<ProgressRule>,
}
```

Key behaviors:

- **Always persists to DB** — no more transcript string accumulation. This gives all
  tasks (including reflection, heartbeat) a queryable message history.
- **Always supports progress tracking** — from TOML frontmatter, no hardcoding.
- **Always injects TODO context** — consistent across all task types.
- **System prompt from definition** — no more `"Job: {name}"` placeholder.

### Progress tracking

The current `ProgressRule` has `min`/`below`/`met`. The `met` message invites early exit
("minimum reached, wrap up") — always harmful, remove it. Keep `min` and rename `below`
to `nudge` as a general-purpose template.

```rust
struct ProgressRule {
    tool: String,
    min: Option<u32>,
    nudge: Option<String>,  // template with {tool}, {count}, {min}
}
```

All fields except `tool` are optional. Behavior matrix:

| `min` | `nudge` | Behavior                                     |
| ----- | ------- | -------------------------------------------- |
| set   | set     | Count shown; nudge printed while count < min |
| unset | set     | Count shown; nudge always printed            |
| set   | unset   | Count shown; nothing else                    |
| unset | unset   | Count shown; nothing else                    |

Once `min` is reached the nudge stops firing. The count line stays — the model sees
`count="5" min="5"` and draws its own conclusions with no "you may wrap up" message.

**Format**: XML inside `<system-reminder>`, matching Claude's convention for injected
system context. This makes it unambiguous to the model that these are runtime-injected
status updates, not user or assistant content.

```xml
<system-reminder>
<progress>
<tool name="web_fetch" count="4" />
<tool name="note_write" count="2" min="3">
Create entity notes for each product/concept found. Do NOT write your handoff yet.
</tool>
<tool name="reference_manage" count="1" />
</progress>
</system-reminder>
```

Self-closing `<tool />` when there is no nudge to display (either no nudge defined, or
min reached). Inner text when the nudge fires.

**TOML examples**:

```toml
# Deep-research: nudge fires only while below 7
[[progress]]
tool = "web_fetch"
min = 7
nudge = "You need at least 7 web_fetch calls before writing your report."

# Reflection: quality self-check — always fires, count gives context
[[progress]]
tool = "note_write"
nudge = "You have created {count} notes so far. Do they capture all important entities from the transcript?"

# Reflection: just track count, no nudge
[[progress]]
tool = "reference_manage"
```

### Prompt context injection

The trigger/scheduling layer is responsible for building the prompt context before
handing off to the execution layer:

- **Reflection**: loads filtered transcript, web cache listing, previous handoff, diary.
  Renders template. Passes rendered prompt as the user message.
- **Heartbeat**: loads session history summary. Passes as user message.
- **Cron jobs**: prompt body from the definition file, optionally with template vars.
- **On-demand agents**: query from the OPERATOR/GHOST, interpolated into prompt body.

This context-building stays in the trigger-specific code (e.g., `ReflectionManager`
still knows how to load web cache). Only the execution itself unifies.

### Background execution

All autonomous tasks run as background tasks via `AgentRunner` (or a renamed
`TaskRunner`). This gives them:

- Cancellation tokens
- Status polling
- Job log tracking
- Session persistence

Interactive chat remains the one case that doesn't use this — `ChatHandler` stays
separate because it has different needs (compaction, Discord message callbacks, etc.).

## Migration plan

### Phase 1: Make reflection an agent definition

- Move reflection prompt from `prompts/reflection.md` to `agents/reflection.md` with
  TOML frontmatter (progress tracking, tools, max_iterations)
- `ReflectionManager` still handles trigger logic (delay, skip-if-no-activity) but calls
  `AgentRunner::start()` instead of `chat_job_with_rules()`
- Delete `reflection_progress_rules()` from Rust — it moves to TOML
- Delete `ToolManager::for_reflection()` — tools come from the definition
- Refactor `ProgressRule` struct: remove `below`/`met`, add optional `min` + `nudge`
- Refactor `build_progress_nudge()` to emit `<system-reminder><progress>` XML format
- **Test**: `deep_research_agent_produces_findings` still passes — validates the XML
  injection format doesn't break the existing agent that relies on progress nudges. If
  there are issues, inform the user, do not change the specs or behaviour.
- **Test**: isolated reflection test still passes

### Phase 2: Make heartbeat an agent definition

- Move heartbeat prompt to `agents/heartbeat.md` with TOML frontmatter
- `HeartbeatManager` still handles trigger logic but uses `AgentRunner`
- Delete `chat_job()` and `JobHandler` — no more callers

### Phase 3: Migrate cron jobs

- Cron jobs already use markdown + TOML frontmatter
- Add `trigger = "cron"` + `schedule` field to agent definition format
- Cron scheduler creates agent sessions instead of calling `chat_job()`
- `JobDefinition` merges into `AgentDefinition` (or renamed `TaskDefinition`)

### Phase 4: Rename agent → task

- `AgentRunner` → `TaskRunner`
- `AgentHandler` → `TaskHandler`
- `AgentDefinition` → `TaskDefinition`
- `agents/` directory → stays as `agents/` (user-facing name is fine)
- Update all references

## What stays separate

- **ChatHandler**: Interactive chat has fundamentally different needs (compaction,
  streaming to Discord, session reuse across messages). It stays as-is.
- **Trigger/scheduling logic**: Each trigger type has its own code for deciding when to
  run and what context to inject. This doesn't unify — it's inherently different per
  trigger.
- **Context building**: Reflection knows about web cache and transcripts. Heartbeat
  knows about idle detection. This domain logic stays in its respective module.

## Key files affected

| File                       | Change                                                         |
| -------------------------- | -------------------------------------------------------------- |
| `src/agents/definition.rs` | Add `trigger`, `delay_seconds` fields                          |
| `src/agents/runner.rs`     | Accept pre-rendered prompts (not just queries)                 |
| `src/jobs/reflection.rs`   | Call `AgentRunner::start()` instead of `chat_job_with_rules()` |
| `src/jobs/heartbeat.rs`    | Call `AgentRunner::start()` instead of `chat_job()`            |
| `src/chat/session.rs`      | Delete `JobHandler`, `chat_job()`, `chat_job_with_rules()`     |
| `agents/reflection.md`     | New file with TOML frontmatter + prompt body                   |
| `agents/heartbeat.md`      | New file with TOML frontmatter + prompt body                   |
| `src/tools/manager.rs`     | Delete `for_reflection()` — tools from definition              |

## Non-goals

- **Lua jobs**: Still a future extension. This spec unifies the Rust execution layer.
  Lua would add a new trigger type and execution backend later.
- **ChatHandler unification**: Interactive chat stays separate. The shared code is
  already in `run_tool_loop()` and `ToolLoopHandler`.
- **Agent continuation for jobs**: Heartbeat and reflection don't need multi-turn
  continuation. They run once and finish. The continuation machinery exists but jobs
  don't use it.
