# Message-to-Source Linking

## Context

The GHOST previously used a `respond` structured output tool to capture citations
alongside messages. The `respond` tool was removed because:

- It added complexity to the tool loop (interception, parsing, special-casing)
- Models sometimes bypassed it, producing plain text EndTurn responses
- The structured citation format was brittle (models omitted fields, used wrong paths)
- EndTurn text output works reliably across all providers

However, the ability to link messages to their sources (web pages, notes, references) is
valuable for:

- Showing the user WHERE an answer came from (clickable source URLs)
- Building a knowledge graph of what sources informed what answers
- Enabling trust/verification workflows ("show me the original page")

## Requirements

1. **Source attribution**: When the GHOST answers using information from fetched web
   pages or knowledge base entries, the UI should show which sources were used.

2. **Graph edges**: Messages should have `cited` edges to `reference` and `note` records
   in SurrealDB, enabling queries like "what sources informed this answer?"

3. **No structured output tool**: The solution should NOT require a special tool call.
   The model should write naturally and sources should be extracted or inferred.

## Possible Approaches

### A. Post-hoc extraction from tool history

After a chat turn completes, scan the message history for `web_fetch` tool calls and
their results. Any URL that was fetched in the same turn (or recent turns) can be
automatically linked as a source. No model cooperation needed.

Pros: Zero prompt engineering, works with any model, no tool loop changes. Cons:
Over-attributes (not every fetched page contributes to the answer).

### B. URL extraction from response text

Parse the assistant's response text for URLs. If a URL matches a previously fetched
page, create a citation edge. The model naturally includes URLs when citing sources.

Pros: Precise — only links sources the model explicitly referenced. Cons: Requires the
model to include URLs (which the prompt can encourage).

### C. Hybrid

Combine A and B: auto-link all fetched URLs, but mark explicitly-mentioned URLs as
"primary" citations.

## Status

Deferred — the current system works without citations. Revisit when the knowledge graph
and UI are more mature.
