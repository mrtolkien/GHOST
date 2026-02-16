# 07 — Context Compaction

## Overview

When a session's message history approaches the model's context window limit, the
compaction system summarizes older messages to free space while preserving key context.

This is the same approach as t-koma — original messages are never deleted, but a
compaction summary replaces them in the prompt.

## How It Works

1. **Trigger**: Before sending to the provider, check if total token estimate exceeds
   `compaction.threshold` (default 85%) of the model's context window.
2. **Select messages**: All messages before the `keep_window` (default: last 20
   messages) are candidates for compaction.
3. **Summarize**: Send the candidate messages to the provider with a compaction prompt
   asking for a structured summary.
4. **Store**: Save the summary to `session.compaction_summary` and set
   `session.compaction_cursor_id` to the last compacted message ID.
5. **Rebuild history**: On next chat, load the summary as a system message, then only
   load messages after the cursor.

## Compaction Prompt

```markdown
Summarize the following conversation history. Preserve:

- Key decisions and their rationale
- Important facts and context established
- Active tasks and their current state
- User preferences and constraints mentioned

Be concise but complete. This summary will replace the original messages in the
conversation context.
```

## Token Estimation

Use a simple heuristic: ~4 characters per token (good enough for English). This avoids
needing a tokenizer dependency. The threshold has enough margin that exact counts aren't
critical.

There should be a TODO: entry saying we will need to implement per-language rules in the
future.

```rust
fn estimate_tokens(text: &str) -> usize {
    text.len() / 4
}
```

## Config

```toml
[compaction]
threshold = 0.85 # Compact when history exceeds 85% of context window
keep_window = 20 # Keep the last 20 messages verbatim
```

## Acceptance Criteria

- Compaction triggers when history exceeds threshold
- Summary is stored in the session record
- Post-compaction, only recent messages + summary are sent to the provider
- Original messages are never deleted
- Compaction produces a tracing span with before/after message counts
- Works correctly across multiple compaction cycles (summary of summary scenario)
- `just ci` passes

## Prior Art

Old code in `../t-koma`:

- `t-koma-gateway/src/session.rs` — Compaction logic (trigger check, summary generation,
  cursor tracking) is embedded in the session module. Directly reusable logic, just
  change the storage calls.
- `prompts/system/compaction-prompt.md` — Compaction prompt text. Reusable as-is.
