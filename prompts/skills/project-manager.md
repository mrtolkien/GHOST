---
name: project-manager
description:
  Create, manage, and organize projects — persistent cross-session task containers for
  long-horizon work. Covers CLI commands, file structure, workflow, and when to create
  or archive projects.
---

# Project Manager — Project Workflow Guide

Projects are persistent, cross-session task containers for long-horizon work. They sit
above the session-scoped `todo` tool. Use projects when work spans multiple days or
sessions.

## When to Create a Project

Create a project when the OPERATOR's request involves:

- **Multi-day work** — tasks that won't finish in a single session
- **Multiple distinct tasks** — 3+ steps with clear dependencies
- **Ongoing tracking** — the OPERATOR wants to see progress over time

**Do NOT** create a project for:

- Quick questions or single-session tasks (use `todo` instead)
- Vague ideas without commitment (discuss first, create later)

**Always ask before creating.** Propose the project structure and get confirmation.

## Project Structure

```
$WORKSPACE/projects/{slug}/
  index.md                # Project description + frontmatter
  tasks/                  # One file per task
    PRIORITY.md           # Ordered list of task slugs (highest first)
    {task-slug}.md        # Task spec
    .archive/             # Completed tasks moved here
  notes/                  # Project-scoped notes
  references/             # Project-scoped reference material
  log.md                  # Append-only progress log with timestamps
```

## CLI Commands

All project management is done via `ghost project` commands through `run_shell_command`.

### Project Commands

```bash
# List projects (default: active only)
ghost project list
ghost project list --status all
ghost project list --status paused

# Create a new project
ghost project init "Project Title" --tags tag1,tag2

# Show project details and task summary
ghost project show {slug}

# Update project status
ghost project status {slug} active|paused|completed

# Archive a project (moves to .archive/)
ghost project archive {slug}

# Add a log entry
ghost project log {slug} "What happened"
```

### Task Commands

```bash
# List tasks in a project
ghost project task list {slug}
ghost project task list {slug} --status todo

# Create a task
ghost project task create {slug} "Task Title"
ghost project task create {slug} "Task Title" --blocked-by dep1,dep2 --body "Details"

# Show full task details
ghost project task show {slug} {task-slug}

# Update task status
ghost project task status {slug} {task-slug} todo|in_progress|done|blocked

# Archive a specific task or all done tasks
ghost project task archive {slug} {task-slug}
ghost project task archive {slug}
```

## Workflow

### Creating a Project

1. Discuss the goal with the OPERATOR
2. Propose a project name and initial tasks
3. Get confirmation before creating
4. Run `ghost project init "Title"` to create the project
5. Run `ghost project task create` for each initial task
6. Add a log entry summarizing the plan

### Working on Tasks

1. Check active projects: `ghost project list`
2. Pick the highest-priority unblocked task
3. Update status: `ghost project task status {slug} {task} in_progress`
4. Do the work
5. Mark done: `ghost project task status {slug} {task} done`
6. Add a log entry with what was accomplished

### Task Priority

Priority is determined by position in `tasks/PRIORITY.md`. New tasks are appended at the
bottom. The OPERATOR can reorder by editing the file directly. Tasks not listed in
PRIORITY.md are treated as unprioritized.

### Archiving

- **Tasks**: Archive individual done tasks or bulk-archive all done tasks
- **Projects**: Archive completed projects to move them out of the active list

### Using the Log

Add log entries at natural milestones — decisions made, tasks completed, blockers hit.
The log provides context for future sessions.

```bash
ghost project log {slug} "Decided to use Cloudflare for DNS. Propagation takes ~24h."
ghost project log {slug} "Homepage design complete. Moving to deployment."
```

## Project Notes and References

Use the `notes/` and `references/` subdirectories within a project for project-scoped
knowledge. These follow the same format as the global knowledge system but are
co-located with the project for easy access.
