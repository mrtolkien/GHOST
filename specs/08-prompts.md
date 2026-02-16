# 08 — System Prompt Rendering

## Overview

The system prompt is assembled from multiple sources and rendered with variable
interpolation. It defines the GHOST's personality, capabilities, and behavioral
guidelines.

## Prompt Stack

The system prompt is composed of these layers (in order):

1. **Base system prompt** — Core instructions shipped with the binary (embedded in code
   or as a resource file)
2. **BOOT.md** — Behavioral directives: how to operate, tool usage guidelines,
   communication rules. Refined by the GHOST from OPERATOR feedback.
3. **SOUL.md** — Personality and identity: name, voice, self-model. Evolves through
   reflection.
4. **OPERATOR.md** — Knowledge about the OPERATOR. Evolves through reflection.
5. **Runtime context** — System info, model info, available skills, today's diary

## Prompt Template

The base system prompt uses `{{ variable }}` interpolation (simple string replacement,
no template engine needed).

Variables:

| Variable           | Source                                            |
| ------------------ | ------------------------------------------------- |
| `ghost_identity`   | BOOT.md (behavior) + SOUL.md (personality)        |
| `operator_context` | Contents of OPERATOR.md                           |
| `ghost_diary`      | Today's diary entry (if any)                      |
| `ghost_commands`   | Core CLI commands (knowledge, web) — specs 11, 13 |
| `ghost_skills`     | List of available skills in `skills/`             |
| `system_info`      | OS, hostname, current time, workspace path        |
| `model_info`       | Current model name and provider                   |

## Identity Files

### BOOT.md — Behavioral Directives

Always-on instructions that drive the GHOST's **behavior**. This is the operational
manual: how to approach tasks, when to use tools, what guidelines to follow, what to
prioritize. Think of it as the GHOST's equivalent of CLAUDE.md — it shapes _how_ the
GHOST operates, not _who_ it is.

Loaded into every session after reboot. Starts with a minimal template and is refined by
the GHOST during reflection in response to OPERATOR feedback and behavioral corrections.

```markdown
# BOOT — Behavioral Directives

## Research First

Always search knowledge and the web before answering factual questions. Don't guess.

When asked to recommend products, search for high quality independant reviews.

## Communication

- Be direct — don't hedge or over-qualify
- Challenge assumptions rather than being sycophantic
- Keep responses concise unless depth is requested
```

### SOUL.md — Personality and Identity

The GHOST's **personality**: its name, voice, communication style, self-model. This is
_who_ the GHOST is as a character. Starts with a minimal template and evolves through
reflection as the GHOST develops self-awareness.

```markdown
# SOUL

## Name

Ghost

## Voice

[Develops through reflection]
```

### OPERATOR.md (starts empty)

Updated by the GHOST during reflection when it learns about the OPERATOR's preferences,
context, and communication style.

## Job and Subsystem Prompts

Heartbeat and reflection are dedicated subsystems with their own prompts (see
[17-default-jobs.md](17-default-jobs.md)). Their prompts are embedded as defaults and
can be overridden by placing `$WORKSPACE/heartbeat.md` or `$WORKSPACE/reflection.md` in
the workspace.

Cron jobs (see [16-jobs.md](16-jobs.md)) use their markdown body as the prompt directly.

The `PromptRenderer` handles variable interpolation (`{{ var }}`) for all prompt types:

### Reflection Prompt Variables

| Variable           | Source                                  |
| ------------------ | --------------------------------------- |
| `previous_handoff` | Contents of `.state/reflection.last.md` |
| `diary_today`      | Today's diary entry from SurrealDB      |
| `recent_messages`  | Filtered session transcript             |
| `web_cache_files`  | File list from `$WORKSPACE/.web-cache/` |

> **Future**: For the PoC, the base system prompt and subsystem prompts are embedded in
> the binary. The end goal is making ALL prompts — including the base system prompt —
> editable as full-text files in the workspace (see `backlog/editable-prompts.md`).

## Implementation

```rust
pub struct PromptRenderer {
    config: Config,
}

impl PromptRenderer {
    /// Render the full system prompt for a chat session.
    #[tracing::instrument(skip_all)]
    pub fn render_system_prompt(&self, context: &PromptContext) -> Result<String>;

    /// Render a job prompt with its specific variables.
    #[tracing::instrument(skip_all, fields(job_name = %job_name))]
    pub fn render_job_prompt(&self, job_name: &str, context: &JobPromptContext) -> Result<String>;
}
```

## Validation

1. `cargo test` — render a system prompt with BOOT.md and SOUL.md present, verify both
   contents appear in the output
2. `cargo test` — render with missing SOUL.md and OPERATOR.md, verify graceful handling
   (empty sections, no error)
3. `cargo test` — `{{ variable }}` interpolation: set `system_info` and `model_info`,
   verify they appear in the rendered prompt
4. `cargo test` — job prompt rendering with custom variables (`{{ previous_handoff }}`,
   `{{ diary_today }}`) produces correct output
5. `just ci` — passes

## Acceptance Criteria

- System prompt is assembled from base + identity files + runtime context
- Missing identity files (SOUL.md, OPERATOR.md) are handled gracefully (empty sections)
- Variable interpolation replaces `{{ var }}` placeholders
- Skills list is populated from `$WORKSPACE/skills/` directory contents
- System info includes current date/time, workspace path
- Job prompts render separately with their own variables
- Prompt rendering produces a tracing span
- `just ci` passes

## Prior Art

Old code in `../t-koma`:

- `prompts/system/system-prompt.md` — Main system prompt text. Directly reusable with
  minor edits (remove t-koma references, update wiki link syntax docs).
- `prompts/system/reflection-prompt.md` — Reflection prompt. Reusable, needs wiki link
  syntax update for typed edges (`[[rel>Target]]`).
- `prompts/system/heartbeat-template.md` — Heartbeat prompt template.
- `prompts/system/compaction-prompt.md` — Compaction prompt.
- `t-koma-gateway/src/prompt/` — Prompt rendering and variable interpolation logic.
