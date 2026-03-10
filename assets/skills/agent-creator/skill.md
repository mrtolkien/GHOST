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

For scheduled/cron agents (e.g., periodic digests, idle reflections), see the
**Scheduled Agents** section below — those live in `$WORKSPACE/agents/` and use
`crontab.lua` instead of skills.

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

---

## Scheduled Agents

Scheduled agents run on a timer (cron expression or idle timeout) rather than being
spawned by a skill. They live directly in `$WORKSPACE/agents/<name>/` and are registered
in `$WORKSPACE/agents/crontab.lua`.

### File Layout

```
agents/
├── crontab.lua               # Schedule registry (required)
├── <agent-name>/
│   ├── agent.lua              # Agent configuration and hooks
│   └── prompt.md              # System prompt template
```

### Crontab Format

`crontab.lua` returns a list of entries. Each entry has a trigger and a `run` field:

```lua
return {
    { idle_minutes = 30, run = "chat-reflection" },
    { cron = "0 7 * * *", run = "daily-digest" },
}
```

- `cron`: Standard 5-field cron expression (`minute hour day month dow`)
- `idle_minutes`: Run after N minutes of user inactivity
- `run`: Agent directory name under `agents/`

### Pre-Fetching with `ctx:call_tool()` and `ctx:call_tools()`

Scheduled agents typically pre-fetch data in `build()` so the LLM only needs to
synthesize — no tools required at runtime. Use `ctx:call_tool(name, args)` for a single
tool call, or `ctx:call_tools({{name, args}, ...})` for batch calls.

- `ctx:call_tool(name, args)` → returns the tool output as a string
- `ctx:call_tools(calls)` → returns two pre-formatted messages:
  - `[1]` = `{ role = "assistant", tool_calls = [...] }`
  - `[2]` = `{ role = "user", tool_results = [...] }`

These messages splice directly into the build return's `messages` list. The LLM sees
them as a natural tool-use conversation.

### Cross-Run State with `ctx:get/set`

Use `ctx:get(key)` and `ctx:set(key, value)` to persist state between runs. This is
useful for deduplication — save a digest summary so the next run can skip
already-reported items.

### Complete Example: Daily Digest Agent

**`agents/daily-digest/agent.lua`:**

```lua
local template = require("ghost.template")

return {
    name = "daily-digest",
    description = "Fetches RSS feeds and produces a daily recap",

    tools = {},          -- LLM has no tools, just synthesizes
    max_iterations = 1,  -- one turn only

    build = function(ctx, args)
        local urls = {
            "https://example.com/feed.xml",
            "https://other.com/news",
        }

        -- call_tools returns pre-formatted messages:
        -- [1] = { role = "assistant", tool_calls = [...] }
        -- [2] = { role = "user", tool_results = [...] }
        local calls = {}
        for _, url in ipairs(urls) do
            table.insert(calls, { "web_fetch", { url = url } })
        end
        local messages = ctx:call_tools(calls)

        -- Append the instruction with previous digest context
        local previous = ctx:get("last_digest") or "No previous digest."
        table.insert(messages, {
            role = "user",
            content = "# Previous digest\n\n" .. previous
                .. "\n\nSummarize the fetched feeds into a concise daily recap. "
                .. "Skip anything already covered in the previous digest.",
        })

        return {
            system_prompt = template.render(read_file("prompt.md"), {
                date = os.date("%Y-%m-%d"),
            }),
            messages = messages,
        }
    end,

    post_completion = function(ctx)
        -- Save the digest so the next run can skip already-reported items.
        local messages = ctx:list_messages(ctx.session_id)
        local last_msg = messages[#messages]
        if last_msg and last_msg.role == "assistant" then
            ctx:set("last_digest", last_msg.content)
        end
    end,
}
```

**`agents/daily-digest/prompt.md`:**

```markdown
# Daily Digest — {{date}}

You are a digest agent. Summarize the fetched web content into a concise daily recap.
Focus on what's new and interesting. Skip anything already covered in the previous
digest provided below.
```

**`agents/crontab.lua` entry:**

```lua
return {
    { cron = "0 7 * * *", run = "daily-digest" },
}
```

### Key Patterns for Scheduled Agents

1. **`tools = {}`** — When the LLM only synthesizes pre-fetched data, give it no tools.
   This prevents unnecessary tool calls and keeps the run to one turn.
2. **`max_iterations = 1`** — Digest agents should produce output in a single turn.
3. **`ctx:call_tools()` in `build()`** — Pre-fetch all data before the LLM runs.
4. **`ctx:get/set` for state** — Persist summaries or timestamps for deduplication.
5. **`post_completion` reads back output** — Use `ctx:list_messages(ctx.session_id)` to
   extract the agent's response and save it for next run.
