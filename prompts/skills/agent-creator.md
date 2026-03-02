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

Design agents that run autonomously with clear purpose and focused tool usage.

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

    -- Build hook (required) — returns system prompt + initial messages
    build = function(ctx, args)
        return {
            system_prompt = template.render(read_file("prompt.md"), {
                date = os.date("%Y-%m-%d"),
            }),
            messages = {
                { role = "user", content = args.prompt or "Begin." },
            },
        }
    end,

    -- Optional hooks
    -- pre_turn = nudges.compose(...),
    -- on_end_turn = nudges.progress_gate(...),
    -- post_completion = function(ctx) end,
    -- should_trigger = function(ctx) return true end,
}
```

## Scheduling

Agents are dispatch-only by default. To schedule your agent, add an entry to
`$WORKSPACE/agents/crontab.lua`:

```lua title="agents/crontab.lua"
return {
    { idle_minutes = 30, run = "chat-reflection" },
    { cron = "0 9 * * 1", run = "weekly-digest" },  -- Monday 9:00 UTC
}
```

Entry types:

- `cron` — 5-field cron expression (UTC)
- `idle_minutes` — trigger after interface sessions idle for N minutes

## Chaining Agents

To chain agents (run one after another), use `ctx:spawn_agent()` in `post_completion`:

```lua
post_completion = function(ctx)
    ctx:spawn_agent("fork-reflection", {
        session_id = ctx.session_id,
    })
end,
```

The child receives the full `args` table in its `build(ctx, args)` hook.

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
    max_iterations = 20,
    tools = { "knowledge_search", "read_file", "write_file", "run_shell_command" },
    build = function(ctx, args)
        return {
            system_prompt = template.render(read_file("prompt.md"), {
                date = os.date("%Y-%m-%d"),
            }),
            messages = {
                { role = "user", content = args.prompt or "Create the weekly digest." },
            },
        }
    end,
}
```

Then add to `agents/crontab.lua`:

```lua
{ cron = "0 9 * * 1", run = "weekly-digest" },
```
