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

Reflection is not a scheduled job. It triggers automatically based on activity:

- **After chat sessions** — when a conversation goes idle
- **After agent research** — immediately when an agent hands off findings

## Chat Reflection

When a chat session has been idle for the configured duration, GHOST spawns a
dedicated `chat-reflection` agent that reviews the conversation transcript.

### What It Produces

| Output | Description |
| --- | --- |
| **Diary entry** | Brief session summary in `diary/{date}.md` — what was discussed, decisions made, open questions |
| **Identity updates** | Updates to `OPERATOR.md`, `BOOT.md`, or `SOUL.md` when the conversation reveals relevant preferences, rules, or personality traits |
| **Notes** | Structured knowledge notes for any information worth preserving long-term |

### How It Triggers

A background watcher polls every 60 seconds. For each active chat session:

1. Check if the session has been idle longer than `reflection_idle_minutes`
2. Check if there are new messages since the last reflection ran
3. If both conditions are met, run chat reflection

The dedup check prevents re-reflecting on the same conversation. The last
reflection's handoff note is stored in `.state/reflection.last.md` — its
file modification time serves as the "last reflected at" marker.

## Agent Reflection (Session Fork)

After an agent completes research, GHOST continues the **same agent session**
with a knowledge extraction prompt. This is called "session forking" — instead
of spawning a separate reflection agent, the research agent switches modes.

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

1. **Agent completes research** — the agent watcher detects the handoff
2. **Web cache classified** — `.web-cache/` files are matched against the
   agent's cited sources (URLs in the findings `## Sources` section)
3. **Fork prompt injected** — a knowledge extraction prompt is appended to
   the existing session, switching the model from research to curation mode
4. **Model writes notes** — using `note_write`, `knowledge_search`, and
   `run_shell_command` to create structured notes following the note-writer
   skill
5. **Post-processing** — cited web cache files are moved to `references/`,
   uncited files are deleted, and `cited` edges are created in the knowledge
   graph linking notes to their source references

### What It Produces

| Output | Description |
| --- | --- |
| **Notes** | Structured knowledge notes with wiki links, archetypes, tags, and source citations |
| **References** | Web cache files promoted to `references/{topic}/{domain}/` |
| **Citation edges** | Knowledge graph edges from notes to the references they cite |

### The Fork Prompt

The fork prompt tells the model to:

1. Discover existing notes (avoid duplicates)
2. Create a TODO plan listing every entity worth writing about
3. Write notes following the note-writer skill conventions
4. Verify completeness against the plan
5. Hand off with a text-only summary

The note-writer skill is inlined directly in the prompt so the model has full
formatting instructions without needing to read a separate file.

## Configuration

```toml title="~/.config/ghost/config.toml"
[timing]
reflection_idle_minutes = 4  # Minutes idle before chat reflection triggers
```

Agent reflection has no timing config — it runs immediately when the agent
hands off.

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

## Serialization

Both chat and agent reflection are serialized through a mutex — only one
reflection can run at a time. This prevents race conditions when multiple
sessions or agents complete close together.
