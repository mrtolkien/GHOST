---
title: Agents
description:
  Lua-defined autonomous workers with configurable triggers, tools, hooks, and nudge
  systems.
---

Agents are Lua-defined autonomous workers that handle complex, multi-step tasks. They
run independently with their own tool sets, iteration limits, system prompts, and
hook-based nudge systems.

## Agent Folder Structure

Each agent lives in `$WORKSPACE/agents/<name>/` with two files:

```
agents/
└── my-agent/
    ├── agent.lua    # Configuration and hooks (required)
    └── prompt.md    # System prompt template (required)
```

## `agent.lua` Contract

```lua title="agents/my-agent/agent.lua"
local nudges = require("ghost.nudges")
local template = require("ghost.template")

return {
    -- Required
    name = "my-agent",
    description = "What this agent does",

    -- Trigger (required)
    trigger = "dispatch",

    -- Model settings (all optional)
    model = nil,              -- nil = default, or "fast", "strong"
    reasoning_effort = nil,   -- nil, "low", "medium", "high"
    max_iterations = 30,      -- max tool loop iterations

    -- Tools and skills
    tools = { "web_search", "web_fetch", "read_file", "todo" },
    skills = { "note-writer" },

    -- System prompt (template interpolation)
    system_prompt = template.render(read_file("prompt.md"), {
        date = os.date("%Y-%m-%d"),
    }),

    -- Optional hooks
    pre_turn = nudges.compose(...),
    on_end_turn = nudges.progress_gate(...),
    -- build_context = function(ctx) return "extra context" end,
    -- post_completion = function(ctx) end,
    -- should_trigger = function(ctx) return true end,
}
```

## Trigger Types

| Trigger       | When it runs                                           | Required fields                       |
| ------------- | ------------------------------------------------------ | ------------------------------------- |
| `dispatch`    | Manual (CLI `ghost agent run` or `agent_control` tool) | —                                     |
| `schedule`    | Cron schedule (UTC, 5-field)                           | `schedule = "0 8 * * *"`              |
| `after_idle`  | After interface sessions idle for N minutes            | `idle_minutes = 30`                   |
| `after_agent` | After another agent completes                          | `continue_trigger_session` (optional) |

### Schedule

```lua
trigger = "schedule",
schedule = "0 8 * * *",   -- 5-field cron, UTC
```

Schedule is interpreted in UTC. Missed runs are skipped when the system is down.

### After Idle

```lua
trigger = "after_idle",
idle_minutes = 30,
```

The unified scheduler polls every `scheduler_tick_seconds` (default 60) and triggers the
agent when any interface session has been idle for the configured duration.

### After Agent

```lua
trigger = "after_agent",
continue_trigger_session = true,  -- optional: continue the completed agent's session
```

When `continue_trigger_session = true`, the after-agent hook continues the completed
agent's existing session instead of starting a fresh one. This preserves the full
research context (see [Reflection](/features/reflection/)).

## Tools

Agents declare which tools they can use via the `tools` list. Available tools:

| Tool                | Description                        |
| ------------------- | ---------------------------------- |
| `web_search`        | Search the web                     |
| `web_fetch`         | Fetch and extract web page content |
| `read_file`         | Read files from the workspace      |
| `write_file`        | Write files to the workspace       |
| `file_edit`         | Edit files in place                |
| `run_shell_command` | Execute shell commands             |
| `knowledge_search`  | Search the knowledge base          |
| `note_write`        | Create knowledge notes             |
| `todo`              | Manage a TODO checklist            |

If an agent declares `skills`, `read_file` is automatically added to its tool set.

## Custom Tools

Agents can define their own tools directly in `agent.lua`:

```lua
custom_tools = {
    {
        name = "my_tool",
        description = "What the tool does",
        parameters = {
            { name = "input", type = "string",
              description = "The input", required = true },
        },
        terminal = false,  -- if true, result ends the session
        handler = function(ctx, args)
            return "tool result"
        end,
    },
},
```

## Hooks

All hooks receive a `ctx` object providing agent context.

| Hook              | When                                     | Return                       |
| ----------------- | ---------------------------------------- | ---------------------------- |
| `build_context`   | Before first turn                        | String to prepend to context |
| `pre_turn`        | Before each model call                   | Nudge message or nil         |
| `on_end_turn`     | After the model final response (EndTurn) | Rejection message or nil     |
| `post_completion` | After agent finishes                     | —                            |
| `should_trigger`  | Before scheduled/idle execution          | Boolean                      |

## Nudge Library (`ghost.nudges`)

The nudge library provides composable functions for `pre_turn` and `on_end_turn` hooks
to keep agents on track.

### `nudges.compose(...)`

Combines multiple nudge functions into one. Each function is called in order; non-nil
results are collected and wrapped in a `<system-reminder>` block.

### `nudges.todo_list`

Injects the current TODO list into the agent's context. Returns the pre-formatted TODO
text from `state.todo_text`, or nil if no TODO exists.

```lua
nudges.todo_list()
```

### `nudges.iteration_countdown`

Count down remaining iterations and nudge the model to wrap up:

```lua
nudges.iteration_countdown({
    { remaining = 10,
      message = "{remaining} iterations left. Prioritize." },
    { remaining = 5,
      message = "Only {remaining} left. Stop new work." },
    { remaining = 2,
      message = "FINAL: {remaining} left. Write your report." },
})
```

### `nudges.temporal`

Fire after a wall-clock duration with optional escalating messages:

```lua
nudges.temporal({
    after_seconds = 300,
    messages = {
        "Working for {minutes} minutes. Start wrapping up.",
        "STOP new work. Write your report NOW.",
    },
})
```

### `nudges.context_pressure`

Fire when context window usage exceeds a percentage threshold:

```lua
nudges.context_pressure({
    threshold_pct = 0.80,
    message = "Context over 80% full. Finish with what you have.",
})
```

### `nudges.progress_gate`

Block end-of-turn until TODO items are completed (for `on_end_turn`):

```lua
nudges.progress_gate({
    no_todo = "REJECTED — create a TODO plan first.",
    incomplete = "REJECTED — {incomplete} items remain.",
})
```

### `nudges.tool_count`

Nudge when a specific tool hasn't been called enough times:

```lua
nudges.tool_count({
    tool = "web_fetch",
    min = 7,
    nudge = "Need {min} {tool} calls (have {count}).",
})
```

### `nudges.recency`

Nudge when a tool hasn't been used in recent turns:

```lua
nudges.recency({
    tool = "web_fetch",
    window = 3,
    message = "You haven't fetched any pages recently.",
})
```

## Default Agents

| Agent               | Trigger       | Purpose                                                     |
| ------------------- | ------------- | ----------------------------------------------------------- |
| **deep-research**   | `dispatch`    | Iterative web research with full page reading               |
| **reflection**      | `after_idle`  | Knowledge extraction from idle chat sessions                |
| **fork-reflection** | `after_agent` | Knowledge extraction by continuing completed agent sessions |

## CLI Commands

```bash
ghost agent list               # List available agents
ghost agent validate <name>    # Validate an agent's Lua config
ghost agent run <name> [prompt] # Run an agent manually
ghost agent logs [name]        # View agent run logs
```

## How Agents Work

1. Agent is triggered (manual dispatch, cron schedule, idle timeout, or agent
   completion)
2. Agent spawns with its own session and restricted tool set
3. Agent runs autonomously, guided by nudge hooks
4. When the agent finishes (text-only response), findings are captured
5. For dispatched agents, findings are injected back into the parent chat
6. After-agent hooks fire for any agents with `trigger = "after_agent"`

## Agent Control (In-Chat)

The GHOST manages agents through the `agent_control` tool during conversations:

```text
start    — Spawn a new agent with name and prompt
continue — Send follow-up instructions to a running agent
status   — Check agent progress
stop     — Terminate an agent
```
