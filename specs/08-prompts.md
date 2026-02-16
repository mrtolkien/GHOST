# 08 — System Prompt Rendering

## Overview

The system prompt is assembled from multiple sources and rendered with variable
interpolation. It defines the GHOST's personality, capabilities, and behavioral
guidelines.

## Prompt Stack

The system prompt is composed of these layers (in order):

1. **Base system prompt** — Core instructions shipped with the binary (embedded in code
   or as a resource file)
2. **BOOT.md** — Core identity, values, behavioral constraints. From workspace.
3. **SOUL.md** — Evolving self-model, communication style. From workspace.
4. **OPERATOR.md** — Knowledge about the OPERATOR. From workspace.
5. **Runtime context** — System info, model info, available skills, today's diary

## Prompt Template

The base system prompt uses `{{ variable }}` interpolation (simple string replacement,
no template engine needed).

Variables:

| Variable           | Source                                     |
| ------------------ | ------------------------------------------ |
| `ghost_identity`   | Concatenation of BOOT.md + SOUL.md         |
| `operator_context` | Contents of OPERATOR.md                    |
| `ghost_diary`      | Today's diary entry (if any)               |
| `ghost_skills`     | List of available skills in `skills/`      |
| `system_info`      | OS, hostname, current time, workspace path |
| `model_info`       | Current model name and provider            |

## Identity Files

### BOOT.md (template for new installations)

```markdown
# BOOT — Core Identity

You are GHOST, a personal AI assistant.

## Values

- Be honest and direct
- Challenge assumptions — don't be sycophantic
- Research before answering
- Be concise
```

### SOUL.md (starts empty)

Updated by the GHOST during reflection when it develops self-awareness insights.

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

## Acceptance Criteria

- System prompt is assembled from base + identity files + runtime context
- Missing identity files (SOUL.md, OPERATOR.md) are handled gracefully (empty sections)
- Variable interpolation replaces `{{ var }}` placeholders
- Skills list is populated from `$WORKSPACE/skills/` directory contents
- System info includes current date/time, workspace path
- Job prompts render separately with their own variables
- Prompt rendering produces a tracing span

## Prior Art

Old code in `../t-koma`:

- `prompts/system/system-prompt.md` — Main system prompt text. Directly reusable with
  minor edits (remove t-koma references, update wiki link syntax docs).
- `prompts/system/reflection-prompt.md` — Reflection prompt. Reusable, needs wiki link
  syntax update for typed edges (`[[rel>Target]]`).
- `prompts/system/heartbeat-template.md` — Heartbeat prompt template.
- `prompts/system/compaction-prompt.md` — Compaction prompt.
- `t-koma-gateway/src/prompt/` — Prompt rendering and variable interpolation logic.
