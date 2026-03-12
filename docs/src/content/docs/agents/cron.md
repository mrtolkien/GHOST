---
title: Cron Jobs
description:
  Centralized scheduling via crontab.lua — cron expressions
  and idle triggers.
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

The scheduler polls once per minute and triggers the agent when an
active interface session has been idle (no new messages) for the
configured duration. Each idle period triggers at most one run per
agent per session — dedup is handled via the `agent_run` table.

```lua
{ idle_minutes = 30, run = "chat-reflection" }
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
