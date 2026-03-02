---
title: Reflection
description:
  Automatic knowledge extraction — how GHOST turns conversations and research
  into structured notes, diary entries, and identity updates.
---

Reflection is GHOST's automatic knowledge extraction layer. After a conversation
goes idle or an agent finishes research, reflection runs to organize what was
learned into persistent knowledge — notes, diary entries, references, and
identity file updates.

Reflection is not a scheduled task in the traditional sense. It triggers
automatically based on activity:

- **After chat sessions** — when a conversation goes idle
- **After agent research** — spawned by the agent's `post_completion` hook

Both reflection types are implemented as [Lua agents](/agents/introduction/).

## Chat Reflection

The `chat-reflection` agent is scheduled via
[`crontab.lua`](/agents/cron/) with `idle_minutes = 30`. It runs
when a chat session has been idle for the configured duration.

### What It Produces

| Output | Description |
| --- | --- |
| **Diary entry** | Brief session summary in `diary/{date}.md` — what was discussed, decisions made, open questions |
| **Identity updates** | Updates to `OPERATOR.md`, `BOOT.md`, or `SOUL.md` when the conversation reveals relevant preferences, rules, or personality traits |
| **Notes** | Structured knowledge notes for any information worth preserving long-term |

### How It Triggers

The unified scheduler polls at the configured tick interval. For each active
interface session:

1. Check if the session has been idle longer than 30 minutes (configured in
   `crontab.lua`)
2. If the threshold is met, run the `chat-reflection` agent

## Agent Reflection (Fork-Reflection)

The `fork-reflection` agent is spawned by `deep-research`'s
`post_completion` hook via `ctx:spawn_agent()`. It receives the
parent's `session_id` in its args and loads the full research
transcript in its `build()` hook using `ctx:list_messages()`.

### Why This Approach

The research agent has the full context of what it found, what sources were
good or bad, what dead ends it hit, and what caveats apply. By loading the
parent session's messages:

- **Full reasoning chain available** — the model sees everything the research
  agent found, including negative evidence and rejected sources
- **Higher quality notes** — empirically produces richer notes with better
  negative-evidence coverage compared to a cold-start reflection

### How It Works

1. **Agent completes** — deep-research finishes its tool loop
2. **`post_completion` fires** — calls
   `ctx:spawn_agent("fork-reflection", { session_id = ctx.session_id })`
3. **fork-reflection starts** — its `build()` loads the parent session's
   messages via `ctx:list_messages(args.session_id)`
4. **Model writes notes** — using `note_write`, `knowledge_search`, and
   `run_shell_command` to create structured notes following the note-writer
   skill
5. **Post-processing** — web cache curation via the `post_completion` hook

### What It Produces

| Output | Description |
| --- | --- |
| **Notes** | Structured knowledge notes with wiki links, archetypes, tags, and source citations |
| **References** | Web cache files promoted to `references/{topic}/{domain}/` |
| **Citation edges** | Knowledge graph edges from notes to the references they cite |

## Configuration

```toml title="~/.config/ghost/config.toml"
[timing]
scheduler_tick_seconds = 60  # How often the scheduler polls
```

The `idle_minutes` for chat-reflection is configured in
`$WORKSPACE/agents/crontab.lua`.

## Reference Curation

Both reflection flows include a reference curation step that manages the web
cache:

1. **Classify** — match each `.web-cache/` file against URLs cited in the
   agent's findings
2. **Curate** — move cited/used files to `references/{topic}/{domain}/`,
   delete uncited files
3. **Link** — create `cited` edges in the knowledge graph connecting notes
   to their source references

This keeps the workspace clean while preserving source material for notes
that reference it.
