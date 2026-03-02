---
title: Context Object
description:
  The ctx object passed to Lua hooks — fields, methods, and usage
  patterns.
---

Every Lua hook receives a `ctx` object (registered as a global)
providing access to agent state, database queries, workspace info,
and agent spawning.

## Fields

| Field | Type | Description |
| --- | --- | --- |
| `ctx.session_id` | `string` | The agent's own session ID |
| `ctx.agent_slug` | `string` | The agent's folder name |
| `ctx.trigger_session_id` | `string\|nil` | Parent session ID (for idle-triggered agents) |
| `ctx.workspace` | `string` | Absolute path to the workspace |

## Methods

### State Store

Agents have a persistent key-value store scoped by agent slug:

```lua
ctx:set("last_run", os.date("%Y-%m-%dT%H:%M:%SZ"))
local val = ctx:get("last_run")  -- string|nil
ctx:delete("last_run")
```

### Session Queries

```lua
-- Count messages in a session since a timestamp
local count = ctx:count_messages_since(session_id, rfc3339_string)

-- List all interface sessions (Discord channels, etc.)
local sessions = ctx:list_interface_sessions()
-- Returns: [{ interface = "discord:channel:123", session_id = "..." }]

-- Get filtered transcript for reflection
local transcript = ctx:filter_transcript(session_id)

-- List all messages in a session
local messages = ctx:list_messages(session_id)
-- Returns: [{ role = "user", content = "...", created_at = "..." }]
```

### Knowledge

```lua
-- Load today's diary file content (or nil if none)
local diary = ctx:load_diary_today()
```

### Web Cache Curation

```lua
-- Classify, curate, and link web cache references
local result = ctx:curate_web_cache()
-- Returns: { moved = N, deleted = N, edges = N }
```

### Agent Spawning

Spawn child agents from hooks or custom tool handlers. The child
receives the full `args` table in its `build(ctx, args)` hook:

```lua
-- In a terminal custom tool handler:
handler = function(ctx, args)
    ctx:spawn_agent("deep-research-reflection", {
        report = args.report,
        sources = args.sources,
    })
    return "Reflection spawned."
end,
```

The child agent runs in the background with a new session and
receives only the structured data passed in args — not the full
parent conversation.

Multiple `ctx:spawn_agent()` calls are allowed — each spawns an
independent child agent.
