---
title: Compaction
description:
  How GHOST manages context window limits with two-phase compaction.
---

Long conversations and agent runs accumulate tokens until they hit the model's context
window limit. Research shows that even full, uncompacted context degrades model
performance as length grows — compaction is a tradeoff, not pure loss.

GHOST uses a two-phase compaction system that balances token recovery with information
preservation.

## Two-Phase Approach

### Phase 1: Tool Result Masking

When estimated token usage exceeds the configured threshold (default: 85% of the context
window), GHOST replaces verbose tool result blocks outside the "keep window" with compact
placeholders like `[tool_result: web_search — first 100 chars... (truncated)]`.

This is free (no LLM call), introduces zero hallucination risk, and recovers thousands
of tokens from large tool outputs. Anthropic recommends this as the first step before
any summarization.

### Phase 2: LLM Summarization

If masking alone isn't sufficient, GHOST summarizes the oldest messages into a structured
summary block via a single LLM call. The summary uses mandatory sections (Task,
Decisions, State, Files, Context) that act as checklists preventing silent information
drops.

The most recent messages (the "keep window") are always preserved in full. Phase 2
errors are logged and gracefully degraded — the masked history is used as a fallback.

## Structured Summaries

GHOST uses a section-based compaction prompt inspired by Factory.ai's benchmark of 36K
production messages, where structured summaries scored 3.70 vs 3.44 for free-form and
3.35 for opaque approaches. The mandatory sections are:

- **Task** — what the OPERATOR asked for
- **Decisions** — key choices with reasoning
- **State** — what was done, what remains
- **Files** — paths read, created, or modified
- **Context** — names, preferences, domain details

The explicit "Files" section addresses a known weakness: all compression methods score
poorly on artifact tracking (2.19–2.45/5.0) without explicit file path sections.

## Configuration

```toml title="~/.config/ghost/config.toml"
[compaction]
threshold = 0.85          # Trigger at 85% context usage
keep_window = 20          # Keep last 20 messages intact
mask_preview_chars = 100  # Characters to preview in masked results
instructions = "Always preserve code snippets in full."  # Optional
```

| Key                  | Default | Description                                    |
| -------------------- | ------- | ---------------------------------------------- |
| `threshold`          | `0.85`  | Context usage ratio that triggers compaction    |
| `keep_window`        | `20`    | Number of recent messages kept in full          |
| `mask_preview_chars` | `100`   | Characters shown in masked tool result previews |
| `instructions`       | —       | Extra text appended to the compaction prompt    |

## Agent Compaction

Agents get different defaults than chat (keep_window=10 vs 20) and full Phase 1 + Phase
2 compaction during tool loops. Agents can override compaction parameters in their Lua
config:

```lua title="agents/my-agent/agent.lua"
return {
    name = "my-agent",
    -- ...
    compaction = {
        keep_window = 8,
        instructions = "Preserve all URLs and the current TODO list. "
            .. "Drop verbose page content.",
    },
}
```

Available overrides: `threshold`, `keep_window`, `mask_preview_chars`, `instructions`.
Any field not specified falls back to the agent default (not the chat default).

## Design Rationale & Sources

- [Factory.ai: Evaluating Context Compression](https://factory.ai/news/evaluating-compression) —
  structured section-based summaries score significantly higher than free-form
- [Anthropic: Effective Context Engineering](https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents) —
  tool result clearing as highest-ROI first step
- [Chroma Research: Context Rot](https://research.trychroma.com/context-rot) —
  full uncompacted context also degrades performance with length
- [Anthropic: Compaction API Docs](https://platform.claude.com/docs/en/build-with-claude/compaction) —
  reference implementation for LLM-driven summarization
