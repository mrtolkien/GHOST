# Compaction Persistence and Write-Time Capping

Companion to `2026-03-23-compaction-overhaul-design.md`. Additive — does not replace or
conflict with the turn-based boundary and overflow recovery work.

## Problem

Phase 1 masking is transient but its effects are permanent. Once a session's token count
crosses the 90% threshold, every future turn reloads full tool results from the
database, re-exceeds the threshold, and re-masks everything before the current turn. The
masking is never persisted, so the system can never "recover" — from that point on, the
LLM sees truncated 100-char previews for all historical tool results, degrading response
quality for the rest of the session.

No cap on tool result size at write time compounds this. A single `web_fetch` or large
file read can store hundreds of thousands of characters in the messages table. One large
result can push the session over the threshold, and even a single past tool result can
dominate the token budget on every reload.

Both issues compound: large uncapped results push sessions over threshold earlier, and
the lack of persistent masking means the system never stabilizes after compaction.

### How other tools handle this

**ZeroClaw** uses destructive in-place mutation — once tool results are masked or
summarized, the originals are gone from the working history. No re-processing on future
turns. Individual tools also cap output at source (1MB for shell, 10MB for file reads).

**OpenClaw** caps tool results at write time (400K chars or 30% of context window) as
the first line of defense. Their in-memory pruning layer (re-applied every turn like
Ghost's phase 1) only operates on already-bounded data. Full LLM compaction is persisted
as a JSONL entry.

Ghost is the only system that keeps full originals in the database and re-processes them
every turn.

## Design

Two independent changes. Can be implemented in either order.

### Change A: Write-time tool result capping

When a tool result is about to be stored in the messages table, enforce a size cap
before persistence.

**Cap logic:**

- A configurable maximum character limit (default TBD — tune based on real session
  data).
- When a tool result exceeds the cap, write the full output to
  `$WORKSPACE/.tool-overflow/{id}.txt`, then replace the stored content with a head+tail
  preview and a pointer to the overflow file.
- Head/tail split: 70% head, 30% tail. The tail often contains error messages,
  summaries, or exit codes that are more useful than the middle.

**Stored content format:**

```
[first N chars of output]

... [full output saved to .tool-overflow/{id}.txt] ...

[last M chars of output]
```

The overflow file is plain text, readable by GHOST via its file tools if it needs the
full content later. Using the workspace directory (not `/tmp`) ensures overflow files
survive reboots and are inspectable by the operator.

**Scope:**

- Applies to tool result text content only. Images and binary content blocks have their
  own handling.
- Tool inputs (arguments to tool calls) are not capped — they are usually small and are
  caught by phase 1 masking if needed.

**Effect:** First line of defense. Prevents pathological cases from ever entering the
DB. Reduces how often compaction triggers.

### Change B: Persistent phase 1 masking

When phase 1 masks a message's tool content, persist the result so future turns don't
re-mask from scratch.

**Mechanism:**

Add a `compacted` boolean column to the messages table (default `false`). When phase 1
runs:

1. For each message before the current turn that has tool content (ToolUse inputs or
   ToolResult blocks), apply the existing masking logic.
2. Write the masked content back to the message row in the database and set
   `compacted = true`.
3. On future loads, messages with `compacted = true` are already masked — no
   re-processing needed.

**Behavioral change:**

- Before: every turn loads full originals, checks threshold, re-masks if over, sends
  masked history. The LLM never sees full tool results from past turns once the session
  has hit the threshold even once.
- After: every turn loads a mix of already-compacted messages and full recent messages,
  checks threshold, only masks the newly-eligible messages (between the last compaction
  point and the current turn), persists those, sends history.

The key difference: after compaction, subsequent turns that stay under the threshold
send full tool results for recent messages. The LLM only loses detail on messages that
were present when compaction actually triggered, not on everything forever.

**Interaction with phase 2 (LLM summarization):**

No change. Phase 2 already persists its summary and a cursor. Messages before the cursor
are never loaded again regardless of their `compacted` state. The `compacted` flag only
matters for messages between the phase 2 cursor and the current turn.

**Interaction with reflection:**

None. Reflection already strips tool results entirely and only uses tool names and input
params. Whether the DB has full or masked content is irrelevant.

**Data loss:**

This is a destructive operation — the original full tool result is replaced in the DB.
This is acceptable because:

- The write-time cap (change A) already means the DB never had the truly full output for
  large results — the overflow file does.
- For results under the cap, the 100-char preview is what the LLM would have seen anyway
  once compaction triggered. Persisting it just makes that permanent.
- Traces in OpenTelemetry remain the audit log for what actually happened.
