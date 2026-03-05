---
title: Project Context
description:
  How the Puppet Master understands your project — repo conventions, skills,
  and context assembly.
---

The Puppet Master's system prompt is assembled from your project's own files.
Every repo can carry its own conventions, and the Puppet Master follows them
automatically.

## Context Sources

The system prompt is built from four sources, in order:

### 1. Base Prompt

A built-in prompt that establishes the Puppet Master's workflow:

1. **Explore** — read the repo structure and key files
2. **Understand** — make sure the task is clear before writing code
3. **Plan** — outline the approach for non-trivial changes
4. **Implement** — make changes incrementally, commit often
5. **Verify** — run tests, linters, and build commands
6. **Report** — summarize what was done

The base prompt emphasizes that the Puppet Master is interactive — it asks
questions, waits for guidance, and doesn't guess.

### 2. Repo Conventions

The Puppet Master reads the first file it finds from the repo root, in this
order:

1. `AGENTS.md`
2. `CLAUDE.md`

This is where you put project-specific rules: coding style, test commands, build
steps, architecture notes, naming conventions. The Puppet Master sees this on
every turn.

:::tip[Write an AGENTS.md]
If your repo doesn't have one, create an `AGENTS.md` at the root. Even a short
file helps:

```markdown
# my-project

## Build & Test
- `npm test` to run tests
- `npm run lint` to check style

## Conventions
- TypeScript strict mode
- Prefer named exports
- Tests next to source files
```
:::

### 3. Skills

The Puppet Master discovers skills from two locations:

- **Workspace skills** (`$WORKSPACE/skills/`) — shared across all GHOST
  sessions
- **Repo-local skills** (`.agents/skills/` in the repo root) — specific to this
  project

Repo-local skills let each project carry specialized workflows. For example, a
project might have a `deploy` skill with deployment steps, or a `migrations`
skill explaining the database migration process.

The Puppet Master sees a list of available skills with their names and
descriptions, and reads the full skill file when a task matches.

### 4. Model Info

The prompt includes which model is being used, so the Puppet Master can adapt
its behavior (e.g., reasoning budget, output length).

## Compaction

Long sessions accumulate context. The Puppet Master uses a tighter
[compaction](/chat/compaction/) config than normal GHOST chat:

| Setting        | GHOST chat | Puppet Master |
| -------------- | ---------- | ------------- |
| `keep_window`  | 20         | 12            |
| `instructions` | generic    | code-aware    |

The code-aware compaction instructions preserve:

- The current plan and TODO checklist status
- All files created, modified, or deleted
- Test results (pass/fail, which tests)
- OPERATOR decisions and preferences
- Current git branch and recent commits
- Errors or blockers encountered

This keeps the Puppet Master focused on the work at hand. Verbose tool outputs
(large file reads, long build logs) get masked first, then older messages get
summarized — but the critical coding state is always retained.
