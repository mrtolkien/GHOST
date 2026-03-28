---
title: Agent Control
description: The agent tool — spawning and managing agents during conversation.
---

The `agent` tool lets your GHOST spawn and manage agents during conversation.

## `agent`

| Action     | Description                            |
| ---------- | -------------------------------------- |
| `start`    | Spawn a new agent with name and prompt |
| `continue` | Send follow-up instructions            |
| `status`   | Check agent progress                   |
| `stop`     | Terminate an agent                     |

When GHOST dispatches an agent, the agent runs autonomously in the background. Once it
completes, its findings are injected back into the chat session as a tool result.

## CLI

You can also manage agents from the command line:

```bash
ghost agent list               # List available agents
ghost agent validate [name]    # Validate agent Lua configs
ghost agent status             # Running agents + recent runs
ghost agent show <run_id>      # View run details and transcript
```

Use `ghost agent status` to see what's running and recent history, then
`ghost agent show <id>` to inspect a specific run. Failed runs
automatically show the error. Add `--full` for the complete message
history, or `--json` for machine-readable output.
