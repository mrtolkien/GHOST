# Backlog — Projects

## Overview

Projects are persistent, cross-session task containers for long-horizon work. They sit
above the session-scoped `todo` tool, which handles short-term task breakdown within a
single run.

The `todo` tool answers: "What are the steps to complete this task right now?" Projects
answer: "What are the open threads across days and sessions?"

## Prior Art

### pi-mono's TODO Extension (mitsuhiko/agent-stuff)

A persistent file-based TODO system for the pi coding agent:

- Each TODO is a markdown file in `.pi/todos/<id>.md` with YAML frontmatter (status,
  tags, timestamps)
- Individual CRUD operations: `create_todo`, `update_todo`, `list_todos`, `read_todo`,
  `delete_todo`, plus `append_todo` for adding notes
- Multi-session: `claim`/`release` mechanism prevents two agent sessions from working on
  the same TODO simultaneously
- Tags for categorization and filtering
- Garbage collection removes completed TODOs after a configurable period
- Rich markdown body — TODOs carry context, not just titles

Key insight: TODOs are **files the agent can read and write**, not opaque database
entries. This aligns with GHOST's text-first philosophy.

### Claude Code Tasks API (v2.1.16+)

Claude Code migrated from session-scoped `TodoWrite` to persistent `Tasks`:

- Four tools: `TaskCreate`, `TaskGet`, `TaskList`, `TaskUpdate`
- Stored on disk at `~/.claude/tasks/<list-id>/`
- Dependency tracking: `blocks`/`blockedBy` relationships between tasks
- Cross-session sharing via `CLAUDE_CODE_TASK_LIST_ID` environment variable
- `owner` field enables multi-agent coordination (one agent creates, another picks up)
- Status: pending → in_progress → completed (or deleted)

Key insight: **dependency tracking** between tasks — not just a flat list. This enables
the agent to reason about what's blocked and what's actionable.

### GitHub Copilot (Remote Coding Agent)

Sidesteps the problem entirely by using **GitHub Issues and sub-issues** as the task
backend. Tasks are inherently persistent, collaborative, and visible to humans. The
agent creates PRs linked to issues.

Key insight: for collaborative/visible work, use the **platform's native project
management** rather than reinventing it.

## Design Direction for GHOST

Projects in GHOST would combine ideas from all three:

- **Text-first** (pi-mono): projects and tasks live as files in
  `$WORKSPACE/projects/<name>/`, readable and editable by both the GHOST and OPERATOR
- **Graph-connected** (unique to GHOST): project tasks link to knowledge notes via wiki
  links (`[[depends_on>SQLite migration]]`), making the knowledge graph aware of active
  work
- **CLI-managed**: `ghost project list`, `ghost project create`, `ghost project status`
  — the GHOST uses these via `run_shell_command`, no dedicated tool needed
- **Reflection-aware**: the reflection subsystem can review open projects and update
  status, add notes, or flag stale items

## Relationship to the `todo` Tool

The two systems serve different purposes and coexist:

| Aspect      | `todo` tool                     | Projects                        |
| ----------- | ------------------------------- | ------------------------------- |
| Scope       | Single session/job              | Cross-session, multi-day        |
| Lifetime    | Dies with the session           | Persists until completed        |
| Granularity | "Search for 3D printer reviews" | "Research and buy a 3D printer" |
| Storage     | DB column (`session.todo_list`) | Files in `$WORKSPACE/projects/` |
| Access      | Dedicated tool (5 actions)      | CLI commands via bash           |
| Created by  | GHOST mid-task                  | GHOST or OPERATOR               |

A project might spawn multiple sessions, each with their own `todo` list for the
immediate work.

## Open Questions

- Should projects have sub-tasks, or is a flat list per project enough?
- Should the GHOST auto-create projects from multi-session conversations, or only when
  explicitly asked?
- How do projects relate to diary entries? (Timeline in diary, structure in project)
- Should projects integrate with external systems (GitHub Issues) or stay local?
