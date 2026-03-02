---
title: Syntax Reference
description:
  Full agent.lua contract — all fields, tools, custom tools, hooks,
  and the nudge library.
---

## `agent.lua` Contract

```lua title="agents/my-agent/agent.lua"
local nudges = require("ghost.nudges")   -- optional
local template = require("ghost.template")

return {
    -- Required
    name = "my-agent",
    description = "What this agent does",

    -- Model settings (all optional)
    model = nil,              -- nil = default, or "fast", "strong"
    reasoning_effort = nil,   -- nil, "low", "medium", "high"
    max_iterations = 30,      -- max tool loop iterations

    -- Tools and skills
    tools = { "web_search", "web_fetch", "read_file", "todo" },
    skills = { "note-writer" },

    -- Hooks (all optional except build)
    build = function(ctx, args) ... end,       -- required
    pre_turn = nudges.compose(...),            -- before each turn
    on_end_turn = nudges.progress_gate(...),   -- gate final response
    post_completion = function(ctx) end,       -- after agent finishes
    should_trigger = function(ctx) return true end, -- for scheduled
}
```

## Tools

Agents declare which tools they can use via the `tools` list:

| Tool | Description |
| --- | --- |
| `web_search` | Search the web |
| `web_fetch` | Fetch and extract web page content |
| `read_file` | Read files from the workspace |
| `write_file` | Write files to the workspace |
| `file_edit` | Edit files in place |
| `run_shell_command` | Execute shell commands |
| `knowledge_search` | Search the knowledge base |
| `note_write` | Create knowledge notes |
| `todo` | Manage a TODO checklist |
| `agent_control` | Start, stop, and check agents |

If an agent declares `skills`, `read_file` is automatically added.

## Custom Tools

Define agent-specific tools directly in `agent.lua`:

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

| Hook | When | Return |
| --- | --- | --- |
| `build` | Before first turn | `{ system_prompt, messages }` |
| `pre_turn` | Before each model call | Nudge message or nil |
| `on_end_turn` | After model's final response | Rejection message or nil |
| `post_completion` | After agent finishes | — (may call `ctx:spawn_agent`) |
| `should_trigger` | Before scheduled/idle execution | Boolean |

### `build(ctx, args)`

Required. Returns the system prompt and initial messages:

```lua
build = function(ctx, args)
    return {
        system_prompt = template.render(read_file("prompt.md"), {
            date = os.date("%Y-%m-%d"),
        }),
        messages = {
            { role = "user",
              content = args.prompt or "Begin." },
        },
    }
end,
```

The `args` table comes from the caller — for dispatch agents it
contains `{ prompt = "..." }`, for spawned agents it contains
whatever the parent passed to `ctx:spawn_agent()`.

### `post_completion(ctx)`

Runs after the agent finishes. Use it to spawn child agents:

```lua
post_completion = function(ctx)
    ctx:spawn_agent("fork-reflection", {
        session_id = ctx.session_id,
    })
end,
```

## Nudge Library (`ghost.nudges`)

The nudge library provides composable functions for `pre_turn` and
`on_end_turn` hooks.

### `nudges.compose(...)`

Combines multiple nudge functions into one. Each function is called
in order; non-nil results are collected and wrapped in a
`<system-reminder>` block.

### `nudges.todo_list()`

Injects the current TODO list into the agent's context.

### `nudges.iteration_countdown`

Count down remaining iterations:

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

Fire after a wall-clock duration with escalating messages:

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

Fire when context window usage exceeds a threshold:

```lua
nudges.context_pressure({
    threshold_pct = 0.80,
    message = "Context over 80% full. Finish with what you have.",
})
```

### `nudges.progress_gate`

Block end-of-turn until TODO items are completed:

```lua
nudges.progress_gate({
    no_todo = "REJECTED — create a TODO plan first.",
    incomplete = "REJECTED — {incomplete} items remain.",
})
```

### `nudges.tool_count`

Nudge when a tool hasn't been called enough:

```lua
nudges.tool_count({
    tool = "web_fetch",
    min = 7,
    nudge = "Need {min} {tool} calls (have {count}).",
})
```

### `nudges.recency`

Nudge when a tool hasn't been used recently:

```lua
nudges.recency({
    tool = "web_fetch",
    window = 3,
    message = "You haven't fetched any pages recently.",
})
```

## `prompt.md` Template

Write a focused system prompt using `{{variable}}` interpolation:

```markdown
# Agent Name — Purpose

You are in autonomous mode. Today is {{date}}.

**A text-only response (no tool calls) ends your session.**

## Workflow

1. First step
2. Second step
3. Handoff (text-only final message)
```
