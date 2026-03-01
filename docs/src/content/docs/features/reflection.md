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

Reflection is not a scheduled task. It triggers automatically based on activity:

- **After chat sessions** — when a conversation goes idle
- **After agent research** — immediately when an agent completes

Both reflection types are implemented as Lua agents with appropriate triggers.

## Chat Reflection

The `reflection` agent has `trigger = "after_idle"` and runs when a chat
session has been idle for the configured duration.

### What It Produces

| Output | Description |
| --- | --- |
| **Diary entry** | Brief session summary in `diary/{date}.md` — what was discussed, decisions made, open questions |
| **Identity updates** | Updates to `OPERATOR.md`, `BOOT.md`, or `SOUL.md` when the conversation reveals relevant preferences, rules, or personality traits |
| **Notes** | Structured knowledge notes for any information worth preserving long-term |

### How It Triggers

The unified scheduler polls at the configured tick interval. For each active
interface session:

1. Check if the session has been idle longer than `idle_minutes` (default 30)
2. If the threshold is met, run the `reflection` agent

## Agent Reflection (Session Fork)

The `fork-reflection` agent has `trigger = "after_agent"` with
`continue_trigger_session = true`. After an agent completes, the
**same session** continues with a knowledge extraction prompt.

### Why Forking

The research agent has the full context of what it found, what sources were
good or bad, what dead ends it hit, and what caveats apply. By continuing the
same session:

- **Full reasoning chain preserved** — the model remembers everything it
  researched, including negative evidence and rejected sources
- **Prompt cache stays warm** — the entire research history is already cached,
  so the reflection phase costs mostly cached input tokens
- **Higher quality notes** — empirically produces ~70% more notes with richer
  negative-evidence coverage compared to a separate reflection agent

### How It Works

1. **Agent completes** — the agent watcher detects the handoff
2. **After-agent trigger fires** — the `fork-reflection` agent is found
3. **Session continues** — because `continue_trigger_session = true`, the
   completed agent's session continues with the knowledge extraction prompt
4. **Model writes notes** — using `note_write`, `knowledge_search`, and
   `run_shell_command` to create structured notes following the note-writer
   skill
5. **Post-processing** — web cache curation via the `post_completion` hook

The `should_trigger` hook skips execution when the completed agent is itself
a reflection agent (prevents reflection loops).

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

The `reflection` agent's `idle_minutes` field controls when chat reflection
triggers. Agent reflection runs immediately when any agent completes.

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
