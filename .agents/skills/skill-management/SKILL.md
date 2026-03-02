---
name: skill-management
description: >-
  Self-documentation for Claude Code. MUST READ after implementing complex features,
  making structural changes, or learning non-obvious patterns that would be useful in
  future sessions. Covers: when to create or update .agents/skills/ files, what belongs
  in a skill vs CLAUDE.md vs auto-memory, the skill file format, and the maintenance
  checklist to run before finishing work.
---

# Self-Documentation

You (Claude Code) are the primary consumer of `.agents/skills/`. These files are your
persistent knowledge base for working on this codebase. They survive across sessions and
make you effective from the first turn.

**Core principle**: whenever you learn something useful that doesn't belong in
CLAUDE.md, capture it in a skill so your future self doesn't have to rediscover it.

## When to Create or Update a Skill

### After implementing a complex feature

If the feature has conventions, multi-file coordination, or non-obvious rules — write
them down. Examples of things that warranted a skill:

- Testing harness (`/testing`) — helpers, mock types, readability rules
- Tracing (`/tracing`) — span naming, OTel conventions, `#[instrument]` patterns
- Lua scripting (`/lua-scripting`) — host globals, ctx bindings, sandbox rules
- E2e tests (`/e2e-testing`) — fixture layout, step chaining, iteration workflow

### After making structural changes

If you moved files, renamed types, changed module boundaries, or altered a subsystem's
public API — check if an existing skill references the old names and update it.

### After debugging a non-obvious issue

If you spent significant effort diagnosing something (e.g., "reqwest 0.13 renamed
`rustls-tls` to `rustls`"), that's worth capturing. Future sessions shouldn't repeat the
investigation.

### After discovering a pattern the codebase relies on

If you notice the codebase consistently follows a convention that isn't documented
anywhere (e.g., "all provider errors use retry-after headers"), write it down.

## What Goes Where

| Content                                        | Destination                      | Why                                |
| ---------------------------------------------- | -------------------------------- | ---------------------------------- |
| Project-wide rules everyone must follow        | `CLAUDE.md`                      | Always loaded, high visibility     |
| Subsystem-specific conventions & API reference | `.agents/skills/<name>/SKILL.md` | Loaded on demand via `/skill-name` |
| Session-specific context, task state           | Auto-memory (`memory/MEMORY.md`) | Lightweight, auto-managed          |
| Operator-facing workflows for the GHOST        | `prompts/skills/<name>.md`       | Embedded in binary, for runtime    |

**Rule of thumb**: if the information is needed _every_ session, it goes in CLAUDE.md.
If it's needed _when working on a specific subsystem_, it goes in a skill. If it's
ephemeral or speculative, it goes in auto-memory (or nowhere).

## Skill File Format

Developer skills live in `.agents/skills/<name>/SKILL.md`:

```markdown
---
name: skill-name
description: >-
  One paragraph. Start with what it covers. Then say WHEN to read it (triggers). The
  description is shown in the skill list — make it specific enough that you know when to
  load the full file. Under 1024 chars.
---

# Title

## Section — concrete rules, tables, code examples
```

### Description triggers

The `description` field is what you see in the skill list. Write it so you'll know to
load the skill at the right time. Good triggers:

- "MUST READ before modifying anything in src/scripting/"
- "MUST READ before writing or modifying any test"
- "MUST READ after implementing complex features or making structural changes"

### Content guidelines

- **File paths** — which files to read/modify for this subsystem
- **Contracts** — data formats, type shapes, naming conventions
- **Do/Don't rules** — concrete, actionable, with rationale
- **Patterns** — recurring code shapes with short examples
- **Gotchas** — non-obvious behaviors, known failure modes, migration notes
- Keep skills **under 500 lines**. Split if needed (`testing` + `e2e-testing`).
- Use lowercase kebab-case names: `lua-scripting`, `e2e-testing`

## Runtime Skills (for the GHOST, not for you)

The GHOST runtime also has skills in `prompts/skills/`. These teach the GHOST how to
perform operator tasks. Some are symlinked into `.agents/skills/` so you can read them
too (e.g., `agent-creator`).

| Directory         | Audience          | Loaded by                                             |
| ----------------- | ----------------- | ----------------------------------------------------- |
| `prompts/skills/` | GHOST runtime     | Embedded in binary, installed to `$WORKSPACE/skills/` |
| `.agents/skills/` | Claude Code (you) | Read via `/skill-name` in sessions                    |

When modifying a runtime skill, remember it gets compiled into the binary via
`include_str!()` in `src/skills.rs`.

## Maintenance Checklist

Run this mentally before finishing any significant PR:

1. **Did I change a subsystem that has a skill?** Grep:
   `rg "file_or_type_I_changed" .agents/skills/`. Update any stale references.
2. **Did I learn something non-obvious?** If yes, does it belong in an existing skill or
   a new one?
3. **Are the description triggers still accurate?** Would you find the right skill when
   working on this feature next time?
4. **Remove stale content** — delete sections about removed features, don't comment them
   out.
