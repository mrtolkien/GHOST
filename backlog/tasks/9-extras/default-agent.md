# Default Agent

## Problem

When GHOST needs to delegate work to isolated context (to avoid polluting the main
conversation), it currently needs a specific named agent. There's no general-purpose
"run this in the background" option.

## Proposal

Add a built-in default agent that `agent` uses when no `name` parameter is provided. It
would:

- Use the chat system prompt, reworded for autonomous execution
- Have access to all standard chat tools
- Accept a prompt and respond with text
- Run in isolated context like any other agent

## Use Cases

- Skills could say "spawn a background agent to do X" without needing a dedicated agent
- GHOST could proactively delegate context-heavy work (multiple web fetches, large file
  processing) to avoid polluting the main conversation
- Safety net for ad-hoc background tasks that don't warrant a full agent definition

## Why Not Now

No concrete use case demands it yet. Every current background task has a dedicated agent
with specialized prompts and tools. Adding a generic agent without proven need risks
creating a tool that's "good enough" for everything but optimal for nothing.

## When to Revisit

- If we find ourselves creating thin wrapper agents that just pass prompts through
- If operators frequently want "do this in the background" without a specific agent
- If per-repo skills need lightweight background execution
