# Compaction Overhaul Design

## Problem

The current compaction system uses a fixed 20-message keep window. This causes two
issues:

1. **Keep window too large**: 20 messages can span multiple tool-calling turns with huge
   tool results, wasting context budget on old tool outputs that should have been
   masked.
2. **Keep window too rigid**: Messages just outside the window get summarized, but their
   tool results were already masked by Phase 1 — so the summarizer receives very little
   useful input.
3. **ChatHandler bug**: The main Discord chat path (`ChatHandler`) only runs Phase 1
   masking during tool loops, never escalating to Phase 2 summarization. The other
   handlers (`CodingHandler`, `JobHandler`) use full compaction.
4. **No overflow recovery**: When a provider returns `context_length_exceeded`, the
   error propagates as a chat failure. No retry, no compaction attempt.

## Design

### Turn-based boundary

Replace the fixed `keep_window` (message count) with a dynamic boundary: the **last user
message with text content**. Everything from that message onward is the "current turn"
and stays verbatim. Everything before it is eligible for masking and summarization.

A `find_current_turn_start(messages) -> usize` function scans backwards for the last
`Role::User` message containing a `ContentBlock::Text` (not just tool results). Returns
the index. If no user text message exists, returns 0 (keep everything — compaction
becomes a no-op, which is safe).

After compaction, history looks like:

```
[summary of all prior turns]      <- single message (Phase 2 output)
[last user text message]           <- verbatim
[assistant tool calls + results]   <- verbatim (current turn in progress)
```

### Two-phase compaction (updated)

**Trigger**: Estimated tokens exceed `context_window * threshold` (default 0.90).

**Phase 1 — Mask everything before the current turn:**

- `ToolResult` blocks -> short placeholder with tool name + preview (existing)
- `ToolUse` inputs -> replace JSON input with `{}` (new — saves tokens on large tool
  call arguments)
- `Image` blocks -> `[image: filename]` (existing)
- `Text` blocks, `Thinking` blocks, `RawOutput` blocks -> untouched

Re-check token estimate. If under threshold, done.

**Phase 2 — LLM summarization:**

- Render the masked pre-turn messages to plain text
- **Cap the rendered text** at `MAX_SUMMARIZATION_INPUT_CHARS` (e.g. 50,000 chars ~12K
  tokens) to prevent the summarizer call itself from overflowing. Truncate from the
  beginning (keep the most recent pre-turn content).
- Send to LLM with compaction prompt (max 2048 output tokens, temperature 0.3, reasoning
  effort low)
- Persist summary + cursor to DB. The cursor points to the message just before the
  current turn boundary (`split = find_current_turn_start(messages)`).
- Reload history from DB (summary + current turn)
- Emit a `ToolLoopEvent::CompactionCompleted` event via the existing `EventSender`. The
  Discord interface renders it as a system message:
  `[context compacted — older conversation was summarized to fit the model's context window]`

### Context overflow recovery

When a provider returns a context overflow error:

1. **Detect** via provider-agnostic string matching on the error message, applied
   **inside each provider** at parse time. The matching produces a typed
   `ProviderError::ContextOverflow(String)` variant — the tool loop matches on the enum,
   not strings. Patterns checked:
   - `"exceeds the context window"`, `"context window of this model"`,
     `"maximum context length"`, `"context_length_exceeded"`,
     `"context length exceeded"`, `"too many tokens"`, `"token limit exceeded"`,
     `"prompt is too long"`, `"input is too long"`, `"prompt_length exceeded"`,
     `"input tokens exceed"`

2. **Recover** in the tool loop: catch `ProviderError::ContextOverflow`, force full
   compaction (Phase 1 + 2), retry the request once.

3. **Give up** if the retry also fails: propagate the error to the user. This covers the
   edge case where the current turn itself exceeds the context window (e.g. user pasted
   200KB of text). Compaction can't help because the current turn is preserved verbatim.

Detection is wired into all provider error paths: Codex SSE, Codex JSON, OpenAI OAuth
HTTP 400, OpenAI Compatible HTTP 400, Anthropic HTTP 400, Anthropic SSE errors.

### Where compaction runs

- Before first request of a new turn (`compact_if_needed`)
- Before every request in the tool loop (pre-send check). Yes, this can trigger a Phase
  2 LLM call — intentionally. A summarization call is cheaper than a failed request.
- After every tool iteration (`post_tool_iteration` — now full compaction for ALL
  handlers, fixing the ChatHandler bug)
- On `ContextOverflow` error (forced, then retry)

### Config changes

Remove `keep_window` from `CompactionConfig`. Breaking change (pre-alpha, acceptable).

Also remove the `keep_window: 12` override in `coding_compaction_config()`. The
coding-specific `instructions` (preserving plan state, files modified, test results)
remain — they're still useful for the summarizer.

Remaining config:

- `threshold: f64` (default 0.90)
- `mask_preview_chars: usize` (default 100)
- `instructions: Option<String>` (extra compaction prompt instructions)

### What stays the same

- Token estimation heuristic (bytes/4)
- Compaction prompt content
- Summary persistence to DB (compaction_summary + cursor)
- Graceful degradation on Phase 2 failure (fall back to masked history)
- Reasoning/thinking blocks preserved throughout
