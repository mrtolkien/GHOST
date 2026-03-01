---
name: agent-creator
description:
  Create and improve Lua-defined agents. Use when the OPERATOR asks for background
  automation, scheduled tasks, or new agents in $WORKSPACE/agents/.
triggers:
  - create an agent
  - schedule a task
  - automate this
  - new agent
---

# Agent Creator

Use this skill when the OPERATOR asks for background automation or scheduled tasks.

## Goal

Design agents that run autonomously with clear trigger conditions and focused tool
usage.

## Where Agents Live

Each agent is a folder in `$WORKSPACE/agents/<name>/` containing:

- `agent.lua` — configuration and hooks (required)
- `prompt.md` — system prompt template (required)

## `agent.lua` Contract

```lua
local nudges = require("ghost.nudges")   -- optional
local template = require("ghost.template")

return {
    -- Required
    name = "my-agent",
    description = "What this agent does",

    -- Trigger (required) — determines when the agent runs
    trigger = "dispatch",           -- manual via CLI or agent_control tool
    -- trigger = "schedule",        -- cron-scheduled
    -- schedule = "0 8 * * *",      -- 5-field cron (UTC), required when trigger = "schedule"
    -- trigger = "after_idle",      -- runs after interface sessions go idle
    -- idle_minutes = 30,           -- required when trigger = "after_idle"
    -- trigger = "after_agent",     -- runs after another agent completes
    -- continue_trigger_session = false, -- if true, continues the completed agent's session

    -- Model settings
    model = nil,                    -- nil = use default, or "fast", "strong", etc.
    reasoning_effort = nil,         -- nil, "low", "medium", "high"
    max_iterations = 30,            -- max tool loop iterations

    -- Tools available to the agent
    tools = {
        "web_search", "web_fetch", "read_file", "write_file",
        "file_edit", "run_shell_command", "knowledge_search",
        "note_write", "todo",
    },

    -- Skills to inject (read via file tools)
    skills = { "note-writer" },

    -- System prompt (use template.render for variable interpolation)
    system_prompt = template.render(read_file("prompt.md"), {
        date = os.date("%Y-%m-%d"),
    }),

    -- Optional hooks (all receive a ctx object)
    -- build_context = function(ctx) return "extra context" end,
    -- pre_turn = function(ctx, state) return nil end,
    -- on_end_turn = function(ctx, state) return nil end,
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

## Nudge Library (`ghost.nudges`)

Nudges inject guidance into the agent's tool loop to keep it on track:

```lua
pre_turn = nudges.compose(
    nudges.todo_list(),
    nudges.iteration_countdown({
        { remaining = 5, message = "Only {remaining} iterations left. Wrap up." },
    }),
    nudges.temporal({
        after_seconds = 300,
        messages = { "You've been working for {minutes} minutes. Start wrapping up." },
    }),
    nudges.context_pressure({
        threshold_pct = 0.80,
        message = "Context window over 80% full. Write your final report.",
    })
),

on_end_turn = nudges.progress_gate({
    no_todo = "REJECTED — create a TODO plan before proceeding.",
    incomplete = "REJECTED — you have {incomplete} incomplete TODO item(s).",
}),
```

Available nudges: `todo_list`, `iteration_countdown`, `temporal`, `context_pressure`,
`progress_gate`, `tool_count`, `recency`.

## Custom Tools

Define agent-specific tools directly in `agent.lua`:

```lua
custom_tools = {
    {
        name = "my_tool",
        description = "What the tool does",
        parameters = {
            { name = "input", type = "string", description = "The input", required = true },
        },
        terminal = false, -- if true, tool result ends the session
        handler = function(ctx, args)
            return "tool result"
        end,
    },
},
```

## `prompt.md` Template

Write a focused system prompt. Use `{{variable}}` for template interpolation:

```markdown
# Agent Name — Purpose

You are in autonomous mode. Today is {{date}}.

**A text-only response (no tool calls) ends your session.**

## Workflow

1. First step
2. Second step
3. Handoff (text-only final message)
```

## Validate Before Finishing

```
ghost agent validate <name>
```

## Example: Weekly Digest Agent

`agents/weekly-digest/agent.lua`:

```lua
local template = require("ghost.template")

return {
    name = "weekly-digest",
    description = "Weekly summary of activity and knowledge",
    trigger = "schedule",
    schedule = "0 9 * * 1",  -- Monday 9:00 UTC
    max_iterations = 20,
    tools = { "knowledge_search", "read_file", "write_file", "run_shell_command" },
    system_prompt = template.render(read_file("prompt.md"), {
        date = os.date("%Y-%m-%d"),
    }),
}
```
