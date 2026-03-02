---
title: Introduction
description:
  What Lua agents are, how they work, and the default agents shipped
  with GHOST.
---

Agents are Lua-defined autonomous workers that handle complex,
multi-step tasks. They run independently with their own tool sets,
iteration limits, system prompts, and hook-based nudge systems.

## Agent Folder Structure

Each agent lives in `$WORKSPACE/agents/<name>/` with two files:

```
agents/
└── my-agent/
    ├── agent.lua    # Configuration and hooks (required)
    └── prompt.md    # System prompt template (required)
```

## Simple Example

```lua title="agents/chat-reflection/agent.lua"
local template = require("ghost.template")

return {
    name = "chat-reflection",
    description = "Reflection on operator chat sessions",
    max_iterations = 30,
    tools = {
        "run_shell_command", "read_file", "write_file",
        "file_edit", "knowledge_search", "note_write",
    },
    skills = { "knowledge-navigator", "note-writer" },
    build = function(ctx, args)
        return {
            system_prompt = template.render(read_file("prompt.md"), {
                date = os.date("%Y-%m-%d"),
            }),
            messages = {
                { role = "user",
                  content = args.prompt or "Begin reflection." },
            },
        }
    end,
}
```

## Default Agents

GHOST ships three built-in agents, installed into
`$WORKSPACE/agents/` on workspace bootstrap:

| Agent | Purpose |
| --- | --- |
| **deep-research** | Iterative web research with full page reading and source evaluation |
| **chat-reflection** | Knowledge extraction from idle chat sessions (scheduled via `crontab.lua`) |
| **fork-reflection** | Knowledge extraction from completed agent sessions (spawned by deep-research's `post_completion`) |

## How Agents Work

1. Agent is triggered (manual dispatch, cron schedule, idle timeout,
   or spawned by another agent's `post_completion`)
2. The `build(ctx, args)` hook produces a system prompt and initial
   messages
3. Agent runs autonomously with its restricted tool set, guided by
   nudge hooks
4. When the agent finishes (text-only response), findings are captured
5. For dispatched agents, findings are injected back into the parent
   chat session
6. The `post_completion` hook runs — it can spawn child agents via
   `ctx:spawn_agent()`

## Running Agents

```bash
ghost agent list               # List available agents
ghost agent validate <name>    # Validate an agent's Lua config
ghost agent run <name> [prompt] # Run an agent manually
ghost agent logs [name]        # View agent run logs
```

During conversation, GHOST can also spawn and manage agents with the
[`agent_control`](/agents/agent-control/) tool.
