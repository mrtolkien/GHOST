---
name: agent-creator
description:
  Create spawnable Lua agents coupled with skills. Use when you need to create an
  autonomous agent that achieves a specific task. Also use when the OPERATOR wants a
  reusable background agent.
---

# Agent Creator

Use this skill when the OPERATOR wants to create a new background agent.

## Core Principle

**Every spawnable agent must live inside a skill.** Skills provide discoverability —
without a skill, an agent is invisible. This skill always creates a skill+agent pair.

For scheduled/cron agents (e.g., periodic digests, idle reflections), those live in
`$WORKSPACE/agents/` and are triggered by `crontab.lua` — that's a different workflow,
not covered here.

## File Layout

```
skills/<skill-name>/
├── skill.md              # Describes when/how to use the agent
└── <agent-name>/
    ├── agent.lua          # Configuration and hooks (required)
    └── prompt.md          # System prompt template (required)
```

A skill can contain multiple agents (see `superpowers/subagent-development/` for an
example with 4 coding agents).

## Step 1: Create the Skill

Write `skills/<skill-name>/skill.md` with frontmatter:

```markdown
---
name: <skill-name>
description:
  <When should GHOST read this skill? Be specific about the trigger conditions.> <Do NOT
  describe *what* it does, focus on WHY this skill should be read.>
---

# <Skill Name>

<Explain what the agent does, when to use it, and how to spawn it.>

## Spawning

\`\`\` agent_control(action: "start", agent: "<agent-name>", prompt:
"<what to include>") \`\`\`
```

The skill's description is what makes GHOST discover and read it. Write it like a
trigger condition — "Use when the OPERATOR asks about X" or "Read when Y happens."

## Step 2: Create the Agent

### `agent.lua` Contract

```lua
local template = require("ghost.template")

return {
    -- Required
    name = "<agent-name>",
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
}
```

### `prompt.md` Template

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

## Step 3: Validate

```
ghost agent validate <agent-name>
```

## Nudge Library (`ghost.nudges`)

Nudges inject guidance into the agent's tool loop:

```lua
local nudges = require("ghost.nudges")

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

## Chaining Agents

To chain agents, use `ctx:spawn_agent()` in a terminal custom tool handler:

```lua
custom_tools = {
    {
        name = "report_findings",
        description = "Submit findings and spawn reflection",
        parameters = {
            { name = "report", type = "string", required = true },
        },
        terminal = true,
        handler = function(ctx, args)
            ctx:spawn_agent("reflection-agent", {
                report = args.report,
            })
            return "Reflection spawned."
        end,
    },
},
```
