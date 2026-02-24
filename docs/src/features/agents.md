# Agents

Agents are autonomous background workers that handle complex, multi-step tasks. They run
independently with their own tool sets, iteration limits, and system prompts.

## Agent Format

Each agent is a markdown file with TOML frontmatter in `$WORKSPACE/agents/`:

```markdown
+++
description = "What this agent does"
tools = ["web_search", "web_fetch", "read_file", "todo"]
max_iterations = 50
model = "primary"
skills = ["knowledge-navigator"]

[[progress]]
rule = "nudge"
tool = "web_fetch"
min_calls = 7
check_at_iteration = 15
message = "You must fetch at least 7 pages."
+++

# Agent System Prompt

Detailed instructions for the agent's behavior...
```

### Key Fields

| Field            | Purpose                                                |
| ---------------- | ------------------------------------------------------ |
| `description`    | Shown in system prompt for agent selection             |
| `tools`          | Whitelist of allowed tools                             |
| `max_iterations` | Hard cap on tool loop iterations                       |
| `model`          | Model alias to use (optional, defaults to primary)     |
| `skills`         | Skills available to this agent                         |
| `progress`       | Runtime rules to enforce behavior (nudges, hard stops) |

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

```
start    — Spawn a new agent with a prompt
continue — Send follow-up instructions to a running agent
status   — Check agent progress
stop     — Terminate an agent
```
