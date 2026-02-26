# 12 — Skills System (agentskills.io)

## Overview

Skills are structured instruction files that guide the GHOST through complex workflows.
They follow the agentskills.io specification and live in `$WORKSPACE/skills/`.

The GHOST reads skills using standard file tools (`read_file`). The system prompt lists
available skills so the GHOST knows what's available.

## Skill Format (agentskills.io)

Each skill is a directory with a `skill.md` file:

```
skills/
├── note-writer/
│   └── skill.md
├── reference-researcher/
│   └── skill.md
├── cron-job-author/
│   └── skill.md
└── skill-creator/
    └── skill.md
```

### skill.md structure

```markdown
---
name: note-writer
version: 1.0.0
description: Advanced note-writing guidance with archetypes
triggers:
  - write a note
  - create a note
  - note about
---

# Note Writer

[Detailed instructions for the workflow...]
```

The frontmatter provides metadata for discovery. The body is the actual skill content
that guides the GHOST.

## Discovery

On startup (and when rendering the system prompt), scan `$WORKSPACE/skills/` and list
available skills:

```
## Available Skills

- **note-writer** — Advanced note-writing guidance with archetypes
- **reference-researcher** — Research and import strategies for reference topics
- **cron-job-author** — Create and improve scheduled jobs
- **skill-creator** — Guide for creating new skills
```

This is injected into the system prompt via the `{{ ghost_skills }}` variable.

## Default Skills

Ship these skills as templates that are copied to the workspace on first run:

1. **knowledge-search** — Advanced knowledge search options: type filters, tag filters,
   output formats, score thresholds
2. **knowledge-graph** — Graph traversal: edge type filters, direction filters, listing
   all edge types (`--types`), finding orphan notes (`--orphans`), interpreting stats
3. **web-search** — Advanced web search and fetch options: result count, freshness, raw
   HTML mode, CSS selector extraction, cache control
4. **note-writer** — Guides the GHOST through creating well-structured notes with proper
   archetypes, tags, and wiki links
5. **reference-researcher** — Strategies for importing and organizing reference material
6. **cron-job-author** — How to write job definitions with proper frontmatter
7. **skill-creator** — Meta-skill for creating new skills

## Self-Authoring

The GHOST can create new skills by:

1. Reading the `skill-creator` skill
2. Creating a new directory in `skills/`
3. Writing a `skill.md` file following the format

This is a key feature — the GHOST extends its own capabilities through skills rather
than through new tool implementations.

## Validation

1. `cargo run -- init` on a fresh workspace — default skills (knowledge-search,
   knowledge-graph, web-search, note-writer, reference-researcher, cron-job-author,
   skill-creator) are installed to `$WORKSPACE/skills/`
2. `cargo test` — skill discovery: place a skill in a temp workspace, verify it appears
   in the scanned skill list with correct name and description
3. `cargo test` — `{{ ghost_skills }}` variable in the system prompt contains the
   discovered skill names and descriptions
4. `cargo test` — skill with missing or malformed frontmatter is skipped with a warning
   log, not a crash
5. `just ci` — passes

## Acceptance Criteria

- Skills are discovered from `$WORKSPACE/skills/` at startup
- Available skills are listed in the system prompt
- The GHOST can read skills using `read_file`
- Default skills are installed to the workspace on first run
- The GHOST can create new skills by writing files
- Skill metadata (name, description, triggers) is parsed from frontmatter
- `just ci` passes

## Prior Art

Old code in `../t-koma`:

- `t-koma-core/src/skills.rs` — Skill type definitions and frontmatter parsing. Reusable
  types, though the discovery mechanism changes (no `load_skill` tool).
- `t-koma-core/src/skill_registry.rs` — Skill scanning and registry. Reusable for the
  startup discovery scan.
- `prompts/skills/` — Default skill content (note-writer, reference-researcher,
  cron-job-author, skill-creator). Directly reusable skill text.
