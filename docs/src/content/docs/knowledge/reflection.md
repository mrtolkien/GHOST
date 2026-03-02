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
- **After agent research** — spawned by the agent's `report_findings` handler

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

## Agent Reflection (Deep-Research-Reflection)

The `deep-research-reflection` agent is spawned by the `report_findings`
terminal custom tool in `deep-research`. When deep-research calls
`report_findings`, the tool's handler passes structured data (report,
sources, secondary info, negative info) to the reflection agent via
`ctx:spawn_agent()`.

### Why This Approach

Instead of loading the full research transcript, the reflection agent
receives only curated, structured data from the research agent:

- **Focused input** — the research agent distills its findings into a
  report, sources list, secondary info, and negative evidence
- **Higher quality notes** — the model works from organized data rather
  than parsing a long transcript
- **Lower token cost** — structured data is much smaller than the full
  conversation history

### How It Works

1. **Agent calls `report_findings`** — deep-research submits its final
   report via the terminal custom tool
2. **Handler spawns reflection** — the `report_findings` handler calls
   `ctx:spawn_agent("deep-research-reflection", { report, sources, secondary_info, negative_info })`
3. **deep-research-reflection starts** — its `build()` receives the
   structured data in `args` and renders it into the system prompt
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
