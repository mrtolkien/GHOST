# Agent Context Safety

## Problem

Agents (deep-research, etc.) have no context window protection. The tool loop
accumulates messages in-memory without compaction or size checks. If the model fetches
large web pages — especially with `readability=false` on e-commerce or catalog pages —
the context can exceed the model's limit and the API call fails.

Observed in `deep_research_agent_produces_findings`: two Shopify collection pages
returned 200K chars each of raw JS/tracking code, pushing the final request to 938KB and
triggering `context_length_exceeded`. The same test passes on runs where the model picks
smaller pages — it's non-deterministic.

Interactive chat (`ChatHandler`) already has compaction via `compact_if_needed()`.
Agents (`TaskHandler`) do not.

## Mitigations (not mutually exclusive)

### 1. Cap web_fetch tool result size

Truncate tool results at a hard limit (e.g., 50K chars). The existing truncation in
`web_fetch` applies to the fetched content but the effective cap may be too high for raw
HTML mode. A 200K tool result is never useful.

### 2. Add compaction to the agent tool loop

Check context budget before each API call, same as `chat()` does. This is the proper
architectural fix — agents would summarize older tool results when the context gets
tight, preserving recent work.

### 3. Default readability=true for research agents

Raw HTML is almost never what the agent needs. The deep-research prompt could instruct
the model to prefer `readability=true`, or the tool could default to it when called by
agents.

### 4. Strip junk tags in htmd conversion

The default `htmd::convert()` faithfully converts ALL HTML elements including
`<script>`, `<style>`, `<noscript>`, and `<svg>`. This is the main reason
`readability=false` fetches balloon to 200K chars — the bulk is inline JS and tracking
code.

`htmd` has no automatic content-filtering mode, but `HtmlToMarkdownBuilder` exposes
`skip_tags()` which takes a tag blocklist. Switching `html_to_markdown()` in
`src/web/fetch.rs` from bare `htmd::convert(html)` to:

```rust
HtmlToMarkdown::builder()
    .skip_tags(vec!["script", "style", "noscript", "svg"])
    .build()
    .convert(html)
```

This is orthogonal to readability — it improves the non-readability path without
changing the readability path (which already strips these via dom_smoothie).

## Data

From `e2e-output/` comparison (Feb 23, 2026):

| Run           | Max request size | Tool result total | readability=false fetches | Outcome                 |
| ------------- | ---------------- | ----------------- | ------------------------- | ----------------------- |
| Old (passing) | 730KB            | 338K chars        | 1 page (139K)             | ok                      |
| New (failing) | 938KB            | 811K chars        | 6 pages (779K)            | context_length_exceeded |
| New (retry)   | ~730KB           | ~340K chars       | fewer large pages         | ok                      |

The progress nudge format change (XML vs text, from spec 27) contributes < 0.1% of the
size difference.
