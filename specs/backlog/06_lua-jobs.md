# Backlog — Lua Jobs + Unified Job System

## Overview

Replace or augment markdown-based jobs with Lua scripts for full programmable workflows.
Lua jobs can hold logic, conditionals, loops, and make API calls — enabling much richer
automation than prompt-only jobs.

The Lua jobs system is also the path to **unifying all autonomous behaviors** under a
single job system with event-based triggers.

## Motivation

- Markdown jobs are limited to "send a prompt with a tool set" — no branching, no logic
- Lua provides a lightweight, embeddable scripting language
- Jobs could orchestrate multi-step workflows: check a condition, call the LLM, process
  the result, take action
- Easier integration with hooks and external systems
- **Heartbeat and reflection need event-based triggers and session context**, which are
  too complex for markdown-only jobs but natural in Lua

## Event-Based Triggers

The Lua job system should support event-based triggers in addition to cron:

| Event           | Fires when                                        |
| --------------- | ------------------------------------------------- |
| `session_idle`  | No OPERATOR messages for `delay` duration         |
| `session_start` | A new session begins (first message in a channel) |
| `job_completed` | Another job finishes (specify `job` field)        |
| `daemon_start`  | The daemon process starts up                      |

Trigger fields: `event`, `delay`, `cooldown`, `job`, `skip_if_no_activity`.

Duration format: `15m`, `1h`, `30s`, `2h30m`.

## Heartbeat/Reflection Migration

The PoC builds heartbeat and reflection as dedicated subsystems (see
`17-default-jobs.md`). When Lua jobs ship, migrate them:

### Heartbeat as Lua Job

```lua
job = {
    name = "heartbeat",
    trigger = { event = "session_idle", delay = "4m", cooldown = "30m",
                skip_if_no_activity = true },
    enabled = true,
}

function run(ctx)
    -- ctx.session gives access to the triggering session's history
    local prompt = ctx.read_file("heartbeat.md")
        or ctx.default_prompt("heartbeat")

    local response = ctx.chat_in_session(prompt)

    if response:find("HEARTBEAT_CONTINUE") then
        ctx.log("heartbeat suppressed")
        return
    end

    ctx.notify(response)
end
```

### Reflection as Lua Job

```lua
job = {
    name = "reflection",
    trigger = { event = "job_completed", job = "heartbeat", cooldown = "4m",
                skip_if_no_activity = true },
    enabled = true,
}

function run(ctx)
    local prompt = ctx.read_file("reflection.md")
        or ctx.default_prompt("reflection")

    -- Build context
    local handoff = ctx.read_state("reflection.last.md") or ""
    local diary = ctx.diary_today() or ""
    local transcript = ctx.filtered_transcript()
    local cache_files = ctx.list_web_cache()

    prompt = prompt:gsub("{{ previous_handoff }}", handoff)
    prompt = prompt:gsub("{{ diary_today }}", diary)
    prompt = prompt:gsub("{{ recent_messages }}", transcript)
    prompt = prompt:gsub("{{ web_cache_files }}", cache_files)

    local response = ctx.chat_in_session(prompt, { tools = "reflection" })

    ctx.save_state("reflection.last.md", response)
    ctx.clear_web_cache()
end
```

### Migration Checklist

- [ ] Lua runtime integrated (`mlua`)
- [ ] Event-based trigger system in scheduler
- [ ] `ctx.chat_in_session()` — run LLM in an existing session's context
- [ ] `ctx.notify()` — send to OPERATOR's Discord channel
- [ ] `ctx.filtered_transcript()` — filtered message history
- [ ] `ctx.diary_today()`, `ctx.read_state()`, `ctx.save_state()`
- [ ] `ctx.list_web_cache()`, `ctx.clear_web_cache()`
- [ ] Remove dedicated heartbeat/reflection code paths
- [ ] Ship heartbeat.lua and reflection.lua as default jobs

## Proposed Design

### Job File

```lua
-- $WORKSPACE/jobs/check-deps.lua
job = {
    name = "check-deps",
    schedule = "0 9 * * MON",
    enabled = true,
}

function run(ctx)
    -- Read project files
    local cargo = ctx.read_file("Cargo.toml")

    -- Call the LLM
    local analysis = ctx.chat("Analyze this Cargo.toml for outdated dependencies: " .. cargo)

    -- Conditional logic
    if analysis:find("outdated") then
        ctx.notify("Found outdated dependencies:\n" .. analysis)
    end

    -- Write to knowledge
    ctx.note_write({
        title = "Dependency Check " .. os.date("%Y-%m-%d"),
        body = analysis,
        tags = {"maintenance/dependencies"},
    })
end
```

### Lua Runtime

Use `mlua` crate for Rust-Lua interop. Expose a `ctx` table with functions:

- `ctx.chat(prompt)` — Send a prompt to the LLM (clean context)
- `ctx.chat_in_session(prompt, opts)` — Send a prompt within a session's context
- `ctx.read_file(path)` — Read a file
- `ctx.write_file(path, content)` — Write a file
- `ctx.shell(command)` — Run a shell command
- `ctx.notify(message)` — Send a message to the OPERATOR
- `ctx.note_write(note)` — Create/update a knowledge note
- `ctx.knowledge_search(query)` — Search knowledge
- `ctx.web_fetch(url)` — Fetch a URL
- `ctx.web_search(query)` — Search the web
- `ctx.log(message)` — Log to job transcript
- `ctx.read_state(filename)` — Read from `.state/`
- `ctx.save_state(filename, content)` — Write to `.state/`
- `ctx.diary_today()` — Get today's diary entry
- `ctx.filtered_transcript()` — Get filtered session transcript
- `ctx.list_web_cache()` — List `.web-cache/` files
- `ctx.clear_web_cache()` — Clear `.web-cache/`
- `ctx.default_prompt(name)` — Get embedded default prompt

### Safety

- Lua scripts run in a sandbox (no filesystem access outside workspace)
- Timeout per job execution (configurable, default 5 minutes)
- Memory limit for Lua VM

## Dependencies

- `mlua = { version = "0.10", features = ["lua54", "async", "send"] }`

## Blocked By

- PoC job system (16-jobs.md) must be stable first
- PoC heartbeat/reflection (17-default-jobs.md) must be working to validate the
  migration
- Need real usage patterns from markdown jobs to inform the Lua API design
