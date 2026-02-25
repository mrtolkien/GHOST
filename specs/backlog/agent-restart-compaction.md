# Agent Restart with Compaction

## Problem

Agents (especially deep-research) occasionally fail with EmptyResponse — the model
returns an empty EndTurn despite recovery nudges. The current recovery nudge adds a
system message to the _same_ history and retries, but the context is already "poisoned"
(the model reached a state where it stops producing output). Retrying with identical
history minus the recovery message is deterministic — it fails the same way.

## Idea

On agent failure (EmptyResponse, timeout, etc.), restart the agent with a **compacted**
history instead of the raw full history. The value is the fresh context window, not just
a new message.

### Compaction strategy

1. Take the agent's existing session history (searches, fetches, tool results)
2. Summarize it into a structured handoff: "Here's what you've researched so far: [list
   of fetched URLs with key findings], [TODO state], [what remains]"
3. Start a new tool loop with this compacted context as the initial user message
4. The agent gets a fresh context window with all prior research distilled

### Key insight

The problem isn't that the model lacks information — it's that the raw context (190K+
chars of HTML, tool results, system messages) overwhelms it. Compaction preserves the
knowledge while resetting the context pressure.

## Design sketch

```
fn restart_agent_with_compaction(session, history) -> Result<ChatResult>:
    1. Extract: all fetched URLs, TODO state, key findings from text blocks
    2. Build compacted prompt: "Continue this research. Here's your progress:
       [structured summary]. Your TODO: [current state]. Write your report."
    3. Create new tool loop with compacted prompt as user message
    4. Run with reduced max_iterations (just enough to finish)
```

## Open questions

- Should compaction use an LLM call (summarize history) or be rule-based (extract URLs,
  TODO, last assistant text)?
- How many restart attempts before giving up?
- Should the compacted agent get the same tools or just `todo` + text output?
- Does the existing `continue_task` infrastructure help here, or do we need a fresh
  session?

## When to implement

When EmptyResponse failures are frequent enough to justify the complexity. Current
mitigations (recovery nudge, progress gate, temporal nudge, context pressure) handle
most cases. This is the nuclear option for the remaining ~10-20% failures.
