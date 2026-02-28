---
title: Agents
description:
  Autonomous background workers for complex multi-step tasks with configurable nudge
  systems.
---

Agents are autonomous background workers that handle complex, multi-step tasks. They run
independently with their own tool sets, iteration limits, and system prompts.

## Agent Format

Each agent is a markdown file with YAML frontmatter in `$WORKSPACE/agents/`:

```markdown title="agents/my-agent.md"
---
name: my-agent
description: What this agent does
tools:
  - web_search
  - web_fetch
  - read_file
  - todo
max_iterations: 50
model: primary
skills:
  - knowledge-navigator
progress:
  - tool: web_fetch
    min: 7
    nudge: "Need {min} {tool} calls (have {count}). Keep going."
---

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

:::note Each section is optional — if an agent doesn't declare it, that nudge type
simply doesn't fire. :::

### `progress` — Tool Count & Iteration Countdown Nudges

The `progress` list supports two rule shapes, distinguished by their fields.

#### Tool Count Rules

Track how many times a tool has been called and nudge the model when below a minimum.

```yaml title="Tool count nudge"
progress:
  - tool: note_write
    min: 3
    nudge: "Need {min} {tool} calls (have {count}). Keep going."
```

| Field   | Type         | Description                                                  |
| ------- | ------------ | ------------------------------------------------------------ |
| `tool`  | string       | Tool name to track                                           |
| `min`   | number (opt) | Minimum call count; nudge fires while below                  |
| `nudge` | string (opt) | Message to inject; interpolates `{tool}`, `{count}`, `{min}` |

#### Iteration Countdown Rules

Count down from `max_iterations` and nudge the model as remaining iterations drop. Among
all applicable rules (where remaining &le; threshold), only the most urgent (lowest
`remaining_iterations`) fires. Fires every turn within a band so the model can't ignore it.

```yaml title="Iteration countdown nudge"
progress:
  - remaining_iterations: 10
    message: "{remaining} iterations left. Start wrapping up."
  - remaining_iterations: 5
    message: "Only {remaining} left. Write your report NOW."
  - remaining_iterations: 2
    message: "FINAL WARNING: {remaining} left. Stop calling tools."
```

| Field                  | Type   | Description                                               |
| ---------------------- | ------ | --------------------------------------------------------- |
| `remaining_iterations` | number | Fire when remaining iterations drop to this value or below |
| `message`              | string | Message to inject; interpolates `{remaining}`             |

#### Mixing Both Types

Both rule types can coexist in the same `progress` list:

```yaml title="Mixed progress rules"
progress:
  - tool: web_fetch
    min: 7
    nudge: "Need {min} {tool} calls (have {count})."
  - remaining_iterations: 10
    message: "{remaining} iterations left."
```

### `progress_gate` — Block EndTurn Until TODO Complete

Prevents the agent from ending its turn without a completed TODO list.

```yaml title="Progress gate"
progress_gate:
  no_todo: Create your TODO checklist before writing.
  incomplete: "You have {incomplete} incomplete items. Complete them."
```

| Field        | Type   | Description                                          |
| ------------ | ------ | ---------------------------------------------------- |
| `no_todo`    | string | Fired when no TODO list exists at all                |
| `incomplete` | string | Fired when items remain; interpolates `{incomplete}` |

### `temporal` — Wall-Clock Timer Nudge

Fires after the specified number of seconds and repeats every iteration thereafter.
Supports escalating messages: provide a list of strings where each entry is used in
order, with the last entry repeating for all subsequent fires.

```yaml title="Temporal nudge (single message)"
temporal:
  after_seconds: 300
  message: "You've been working for {minutes} minutes. Wrap up now."
```

```yaml title="Temporal nudge (escalating messages)"
temporal:
  after_seconds: 300
  message:
    - "You've been working for {minutes} minutes. Start wrapping up."
    - "STOP researching. Write your report NOW."
    - "FINAL WARNING. Your next response MUST be your report."
```

| Field           | Type                    | Description                                      |
| --------------- | ----------------------- | ------------------------------------------------ |
| `after_seconds` | number                  | Seconds before the nudge first fires             |
| `message`       | string \| list\<string> | Message(s) to inject; interpolates `{minutes}`.  |
|                 |                         | List entries escalate: index 0 first fire, index |
|                 |                         | 1 second, last entry repeats thereafter.         |

### `recency` — Tool Not Used Recently

Fires periodically when a tool hasn't been used in the last N assistant turns.

```yaml title="Recency nudge"
recency:
  tool: web_fetch
  window: 3
  message: You haven't fetched any pages recently.
```

| Field     | Type   | Description                               |
| --------- | ------ | ----------------------------------------- |
| `tool`    | string | Tool name to check for recent use         |
| `window`  | number | Number of recent assistant turns to check |
| `message` | string | Message to inject when tool is absent     |

### `context_pressure` — Context Size Threshold

Fires once when total conversation content exceeds the character threshold.

```yaml title="Context pressure nudge"
context_pressure:
  threshold_chars: 250000
  message: Context filling up. Finish efficiently.
```

| Field             | Type   | Description                               |
| ----------------- | ------ | ----------------------------------------- |
| `threshold_chars` | number | Character count threshold                 |
| `message`         | string | Message to inject when threshold exceeded |

## Default Agents

| Agent               | Purpose                                       | Max Iterations |
| ------------------- | --------------------------------------------- | -------------- |
| **deep-research**   | Iterative web research with full page reading | 30             |
| **chat-reflection** | Knowledge extraction from chat sessions       | 30             |

Agent reflection (after research) uses [session forking](/features/reflection/)
instead of a dedicated agent — the research session continues in knowledge
extraction mode.

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
