---
title: Agents
description:
  Autonomous background workers for complex multi-step tasks with configurable nudge
  systems.
---

Agents are autonomous background workers that handle complex, multi-step tasks. They run
independently with their own tool sets, iteration limits, and system prompts.

## Agent Format

Each agent is a markdown file with TOML frontmatter in `$WORKSPACE/agents/`:

```markdown title="agents/my-agent.md"
+++
name = "my-agent"
description = "What this agent does"
tools = ["web_search", "web_fetch", "read_file", "todo"]
max_iterations = 50
model = "primary"
skills = ["knowledge-navigator"]

[[progress]]
tool = "web_fetch"
min = 7
nudge = "Need {min} {tool} calls (have {count}). Keep going."
+++

# Agent System Prompt

Detailed instructions for the agent's behavior...
```

### Key Fields

| Field            | Purpose                                            |
| ---------------- | -------------------------------------------------- |
| `name`           | Agent identifier (matches filename without `.md`)  |
| `description`    | Shown in system prompt for agent selection         |
| `tools`          | Whitelist of allowed tools                         |
| `max_iterations` | Hard cap on tool loop iterations                   |
| `model`          | Model alias to use (optional, defaults to primary) |
| `skills`         | Skills available to this agent                     |
| `progress`       | Periodic tool-count nudges (see below)             |

## Nudge Configuration

All model-facing nudge strings live in agent frontmatter.

:::note
Each section is optional — if an agent doesn't declare it, that nudge type
simply doesn't fire.
:::

### `[[progress]]` — Periodic Tool Count Nudges

Track how many times a tool has been called and nudge the model when below a minimum.

```toml title="Progress nudge"
[[progress]]
tool = "note_write"
min = 3
nudge = "Need {min} {tool} calls (have {count}). Keep going."
```

| Field   | Type         | Description                                                  |
| ------- | ------------ | ------------------------------------------------------------ |
| `tool`  | string       | Tool name to track                                           |
| `min`   | number (opt) | Minimum call count; nudge fires while below                  |
| `nudge` | string (opt) | Message to inject; interpolates `{tool}`, `{count}`, `{min}` |

### `[progress_gate]` — Block EndTurn Until TODO Complete

Prevents the agent from ending its turn without a completed TODO list.

```toml title="Progress gate"
[progress_gate]
no_todo = "Create your TODO checklist before writing."
incomplete = "You have {incomplete} incomplete items. Complete them."
```

| Field        | Type   | Description                                          |
| ------------ | ------ | ---------------------------------------------------- |
| `no_todo`    | string | Fired when no TODO list exists at all                |
| `incomplete` | string | Fired when items remain; interpolates `{incomplete}` |

### `[temporal]` — Wall-Clock Timer Nudge

Fires once after the specified number of seconds.

```toml title="Temporal nudge"
[temporal]
after_seconds = 300
message = "You've been working for {minutes} minutes. Wrap up now."
```

| Field           | Type   | Description                                 |
| --------------- | ------ | ------------------------------------------- |
| `after_seconds` | number | Seconds before the nudge fires              |
| `message`       | string | Message to inject; interpolates `{minutes}` |

### `[recency]` — Tool Not Used Recently

Fires periodically when a tool hasn't been used in the last N assistant turns.

```toml title="Recency nudge"
[recency]
tool = "web_fetch"
window = 3
message = "You haven't fetched any pages recently."
```

| Field     | Type   | Description                               |
| --------- | ------ | ----------------------------------------- |
| `tool`    | string | Tool name to check for recent use         |
| `window`  | number | Number of recent assistant turns to check |
| `message` | string | Message to inject when tool is absent     |

### `[context_pressure]` — Context Size Threshold

Fires once when total conversation content exceeds the character threshold.

```toml title="Context pressure nudge"
[context_pressure]
threshold_chars = 250000
message = "Context filling up. Finish efficiently."
```

| Field             | Type   | Description                               |
| ----------------- | ------ | ----------------------------------------- |
| `threshold_chars` | number | Character count threshold                 |
| `message`         | string | Message to inject when threshold exceeded |

## Default Agents

| Agent               | Purpose                                       | Max Iterations |
| ------------------- | --------------------------------------------- | -------------- |
| **deep-research**   | Iterative web research with full page reading | 50             |
| **heartbeat**       | Proactive check-in when you're idle           | 10             |
| **chat-reflection** | Knowledge extraction from chat sessions       | 30             |
| **reflection**      | Knowledge extraction from agent sessions      | 60             |

## How Agents Work

1. GHOST decides an agent is needed (based on context, skills, or user request)
2. Agent spawns via the `agent_control` tool with a prompt
3. Agent runs autonomously with its restricted tool set
4. Findings are injected back into the parent chat
5. GHOST can check status, continue, or stop agents mid-run

## Agent Control

The GHOST manages agents through the `agent_control` tool:

```text title="agent_control actions"
start    — Spawn a new agent with name and prompt
continue — Send follow-up instructions to a running agent
status   — Check agent progress
stop     — Terminate an agent
```
