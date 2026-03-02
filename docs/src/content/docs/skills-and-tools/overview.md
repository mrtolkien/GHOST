---
title: Skills & Tools
description:
  How GHOST uses skills and tools to accomplish tasks — a skill-driven
  architecture.
---

GHOST is **skill-driven**. Rather than encoding workflows in code, GHOST reads plain-text
skill files that teach it how to handle specific tasks — then uses its tools to execute
them.

## Skills vs Tools

| | Skills | Tools |
| --- | --- | --- |
| **What** | Workflow instructions (markdown files) | API functions the model can call |
| **Where** | `$WORKSPACE/skills/` | Built into the binary |
| **Added by** | OPERATOR or GHOST itself | Developers |
| **When used** | Model reads the skill before responding | Model calls the tool during a turn |

The philosophy: **add a skill before adding a tool**. Skills are cheaper to create, easier
to iterate on, and don't require code changes. Tools are for capabilities that need
structured input/output (file I/O, search, web fetch).

## How They Work Together

A typical workflow:

1. You ask your GHOST to research something
2. The system prompt lists available skills — GHOST picks **research**
3. GHOST reads the skill file for step-by-step instructions
4. GHOST executes those steps using tools (`web_search`, `web_fetch`, `note_write`, etc.)

Skills guide *what* to do. Tools provide *how* to do it.

## Available Tools

GHOST ships with tools for:

- **File operations** — `read_file`, `write_file`, `file_edit`
- **Shell access** — `run_shell_command`
- **Task tracking** — `todo`
- **Web research** — `web_search`, `web_fetch` (see [Web Research](/web-research/overview/))
- **Knowledge** — `knowledge_search`, `note_write` (see [Knowledge Tools](/knowledge/tools/))
- **Agent control** — `agent_control` (see [Agent Control](/agents/agent-control/))

See [Core Tools](/skills-and-tools/core-tools/) for file, shell, and task tracking
tool reference.
