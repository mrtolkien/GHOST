# Backlog — Tool Count: Why Fewer Tools First

## Decision

GHOST ships with 5 core tools + 3 reflection-only tools. Knowledge search, web search,
and web fetch are CLI commands accessed via `shell`, with skills providing
usage guidance. This is intentional and evidence-based.

If real usage shows the GHOST frequently botching CLI invocations for core operations
(knowledge search, web search), promote them to dedicated tools. Don't add tools
speculatively.

## Evidence

### Minimal tools match full toolsets on benchmarks

- **Mini-SWE-Agent** (1 tool: bash only): 75.4% on SWE-bench Verified
- **Verdent** (4 tools: bash/read/write/edit): ~76% on SWE-bench Verified
- **Verdent** (full toolset): 76.1% — "very little" difference from minimal
- **DIRECTSOLVE** (0 tools, just context): 50.8% on SWE-bench Verified
- Source: Verdent technical report, Mini-SWE-Agent (github.com/SWE-agent/mini-swe-agent)

### Removing tools improved performance

- **Vercel d0**: went from 17 tools to 2 (bash + SQL)
  - Success rate: 80% → 100%
  - Speed: 3.5x faster
  - Tokens: -40%
  - Quote: "Every tool is a choice you're making for the model. Sometimes the model
    makes better choices."
- Source: vercel.com/blog/we-removed-80-percent-of-our-agents-tools

### Too many tools cause failures

- **Llama 3.1-8b** failed entirely with 46 tools, succeeded with 19
- **Natural Language Tools paper**: replacing JSON tool schemas with natural language
  descriptions gave +18.4pp accuracy and -47% tokens
- Source: arXiv:2411.15399 ("Less is More"), arXiv:2510.14453 ("Natural Language Tools")

### Token overhead is real

- Each tool schema: ~100 tokens, loaded every turn whether used or not
- 58 MCP tools = ~55,000 tokens before the conversation starts
- Claude Code system prompt: 15-17k tokens; pi-mono: <1k tokens
- Source: Anthropic tool use docs, anthropic.com/engineering/advanced-tool-use

### Skills are natural language tool descriptions

- The "Natural Language Tools" paper showed natural language > JSON schemas (+18.4pp)
- Skills are exactly this: a markdown file describing how to use a CLI command
- Loaded on demand (~1,500 tokens when used) vs tool schemas (~100 tokens every turn)
- For a tool used 3x in a 50-turn session, skill costs 4,500 tokens total; a tool schema
  costs 5,000 tokens total — roughly equivalent, but the skill is zero-cost on the 47
  turns where it's not used

## When to Promote CLI → Tool

Promote a CLI command to a dedicated tool when:

1. The GHOST consistently misformats the command (quoting, flags)
2. The operation has structured output that the model needs to parse reliably
3. The operation is called 5+ times per session on average
4. The operation has dangerous side effects where a malformed command could corrupt data
   (this is why `note_write`/`reference_write` are already tools)

## Current Tool Inventory

### Always loaded (5 core)

`shell`, `file_read`, `file_write`, `file_edit`, `todo`

### Reflection only (+2)

`note_write`, `reference_write`

### CLI via bash (candidates for promotion if needed)

`ghost knowledge search`, `ghost knowledge get`, `ghost knowledge graph`,
`ghost web search`, `ghost web fetch`

## NOTES

2026-02-17: in _one_ e2e run, the GHOST failed to call the right web fetch. It got it
right 2/3 times.
