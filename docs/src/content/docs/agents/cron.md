---
title: Cron Jobs
description:
  Centralized scheduling via crontab.lua — cron expressions,
  idle triggers, and how scheduling interacts with should_trigger.
---

Agent scheduling is centralized in a single file:
`$WORKSPACE/agents/crontab.lua`. Agents not listed here are
dispatch-only (run manually or spawned by other agents).

## `crontab.lua` Format

```lua title="agents/crontab.lua"
return {
    { idle_minutes = 30, run = "chat-reflection" },
    -- { cron = "0 3 * * *", run = "daily-summary" },
}
```

Each entry has a `run` field (agent name) and one of:

| Field | Type | Description |
| --- | --- | --- |
| `cron` | `string` | 5-field cron expression (UTC) |
| `idle_minutes` | `number` | Trigger after sessions idle for N minutes |

An entry must have exactly one of `cron` or `idle_minutes`, not both.

## Cron Entries

Standard 5-field cron (minute, hour, day-of-month, month,
day-of-week), interpreted in UTC:

```lua
{ cron = "0 9 * * 1", run = "weekly-digest" }   -- Monday 9:00 UTC
{ cron = "0 3 * * *", run = "daily-summary" }    -- Daily 3:00 UTC
{ cron = "*/30 * * * *", run = "periodic-check" } -- Every 30 min
```

Missed runs are skipped when the system is down.

## Idle Entries

The scheduler polls at `scheduler_tick_seconds` (default 60) and
triggers the agent when any active interface session has been idle
for the configured duration:

```lua
{ idle_minutes = 30, run = "chat-reflection" }
```

## `should_trigger` Interaction

If a scheduled or idle agent defines a `should_trigger(ctx)` hook in
its `agent.lua`, the scheduler calls it before running. Return
`false` to skip execution for this cycle:

```lua
-- In agents/chat-reflection/agent.lua
should_trigger = function(ctx)
    -- Only run if there are recent messages to reflect on
    local count = ctx:count_messages_since(
        ctx.trigger_session_id,
        os.date("!%Y-%m-%dT%H:%M:%SZ", os.time() - 86400)
    )
    return count > 5
end,
```

## Configuration

```toml title="~/.config/ghost/config.toml"
[timing]
scheduler_tick_seconds = 60  # How often the scheduler polls
```

## File Watching

The scheduler watches `$WORKSPACE/agents/` for changes. When
`crontab.lua` or any agent file is modified, the schedule is
automatically reloaded without restarting the daemon.
