# Chat Reflection: Session Fork Approach

## Context

Agent reflection now uses a session-fork approach: instead of spawning a dedicated
reflection agent, we continue the research session with a knowledge-extraction prompt.
This preserves the full reasoning chain, keeps the prompt cache warm, and produces
richer notes with better negative-evidence coverage (validated over 6 runs).

Chat reflection still uses the old dedicated-agent approach (`chat-reflection.md` agent,
new session, synthesized context message).

## Proposal

Evaluate whether chat reflection should also switch to session forking:

1. Continue the chat session with a reflection prompt (diary + identity + notes
   extraction)
2. The model keeps full conversation context instead of a synthesized transcript
3. Prompt cache stays warm from the chat turns

## Considerations

- Chat sessions are typically shorter than research sessions, so the cache/context
  benefit may be smaller
- Chat reflection writes diary entries and identity files, not just notes — the fork
  prompt would need to cover these
- Chat sessions don't have web cache to classify, simplifying the flow
- The chat session's system prompt is different from the reflection agent's — forking
  inherits the chat system prompt, which may not be ideal for reflection tasks

## Evaluation Plan

Run the same A/B comparison we did for agent reflection:

- 3 runs standard, 3 runs fork
- Compare: token cost, diary quality, note quality, identity file updates
