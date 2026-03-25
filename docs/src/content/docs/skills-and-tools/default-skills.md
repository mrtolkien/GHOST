---
title: Default Skills
description: The skills that ship with every GHOST workspace.
---

GHOST installs these skills into `$WORKSPACE/skills/` when you first
set up your workspace. They are kept up to date automatically — every
time GHOST starts, it checks for newer versions bundled with the
binary. If you haven't modified a skill, it updates silently. If both
you and an update changed the same skill, GHOST shows you the diff and
lets you accept, reject, or review each change.

## Research & Knowledge

### deep-research

Multi-source web research for recommendations, comparisons, and buying
decisions. Searches broadly, reads full pages, cross-references findings,
and cites sources. For complex questions, delegates to a background research
agent that takes its time for a thorough, accurate answer.

### knowledge-navigator

Querying the knowledge base effectively. Covers search strategies (keyword
vs semantic), graph traversal, tag browsing, and combining multiple queries
to find what you need.

### note-writer

Creating structured knowledge notes following the note archetype system.
Covers archetypes, wiki links, trust levels, and source citations.

### reference-import

Importing external content — git repositories and website crawls — into the
knowledge base for natural language querying.

### document-import

Importing documents (PDF, DOCX, XLSX, PPTX, images) into the knowledge
base via docling-serve.

## Task Management

### project-manager

Persistent cross-session projects for long-horizon work. Tracks tasks and
progress across conversations. Activates when you describe a goal that will
span multiple sessions.

### scripting

Writing and running scripts to answer questions that need computation —
data processing, CSV/JSON parsing, API calls, calculations, chart
generation. Scripts are saved in the workspace for reuse.

## Content Creation

### image-generation

Generating and editing images with Gemini Flash. Supports text-to-image and
image-to-image at multiple resolutions. Requires `GEMINI_API_KEY`.

### sending-attachments

Sending images, generated files, CSVs, or any attachment back to you.

## Agents & Extensibility

### agent-creator

Creating spawnable Lua agents coupled with skills — autonomous background
workers for specific tasks.

### skill-creator

Meta-skill for creating, editing, and evaluating skills. Ported from
[anthropics/skills](https://github.com/anthropics/skills).

## Development (GHOST HACK)

### coding

Manages coding sessions — loading project context, reading code, running
tests, committing. Powers the GHOST HACK workflow.

### browser-use

Internal skill for multi-browser management, tab workflow, element
references, and authentication handoff.

### superpowers

A collection of development methodology skills ported from
[obra/superpowers](https://github.com/obra/superpowers). These activate
automatically during GHOST HACK coding sessions and cover:

- **brainstorming** — exploring intent and requirements before
  implementation
- **writing-plans** — comprehensive implementation plans before touching
  code
- **executing-plans** — plan execution with review checkpoints
- **tdd** — test-driven development workflow
- **systematic-debugging** — structured debugging before proposing fixes
- **verification** — evidence-based completion checks before committing
- **requesting-review** / **receiving-review** — code review workflows
- **finishing-branch** — merge, PR, or cleanup decisions
- **parallel-agents** / **subagent-development** — dispatching independent
  tasks to parallel workers
- **git-worktrees** — isolated feature work
- **writing-skills** — creating and verifying new skills

## Infrastructure

### system-management

Shell environment (Nix flake), services (start/stop/debug), and self-update.
Merges the former `nix-shell` and `services` skills.
