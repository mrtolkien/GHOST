---
title: Agent Control
description: The agent_control tool — spawning and managing agents during conversation.
---

The `agent_control` tool lets your GHOST spawn and manage agents during conversation.

## `agent_control`

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
ghost agent validate <name>    # Validate an agent's Lua config
ghost agent run <name> [prompt] # Run an agent manually
ghost agent logs [name]        # View agent run logs
```
