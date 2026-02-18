---
name: research
description:
  Research, recommendations, comparisons, buying decisions, product evaluations,
  and any question requiring current web data. Read before searching or
  recommending anything.
---

# Research Skill

## When to Spawn a Deep Research Agent

**ALWAYS** use `agent_control(action: 'start', agent: 'deep-research', ...)` for:

- **Product or service recommendations** — "what X should I buy", "best Y for Z"
- **Comparative analysis** — "A vs B", "compare options for..."
- **Current-data questions** — anything where the answer depends on what's available
  right now (models, prices, specs change constantly)
- **Multi-factor decisions** — questions involving tradeoffs across several dimensions
  (price, quality, features, ecosystem, community)
- **Technology evaluations** — "which framework/tool/platform for my use case"

These questions REQUIRE reading 3-8 full pages, cross-referencing claims, and checking
publication dates. That volume of reading belongs in a background agent, not the main
chat context.

**Do NOT spawn an agent for:**

- Simple factual lookups ("what is the capital of France")
- Questions you can answer from existing knowledge (check knowledge_search first)
- Quick definition or explanation requests
- Questions where 1-2 web fetches are genuinely sufficient

When in doubt, spawn the agent. It's better to over-research than to give shallow advice
from search snippets.

## Follow-Up Questions: ALWAYS Continue the Agent (NON-NEGOTIABLE)

When the OPERATOR provides follow-up criteria, refinements, or new constraints after a
research agent has delivered findings, you **MUST** continue the existing agent session:

```
agent_control(action: 'continue', agent_id: '<id from the original start>',
  prompt: '<new constraints and follow-up question>')
```

**Why?** The agent has already read 5+ pages and built context. Continuing it lets it
refine its answer with targeted follow-up searches instead of starting from scratch.

**NEVER answer a research follow-up directly from the agent's findings.** The findings
are a starting point. If the OPERATOR adds criteria like "actually multicolor is
important" or "my budget is $1000", that changes the recommendation — continue the agent
so it can research the new angle properly.

**NEVER spawn a new agent for follow-ups.** Use `continue` on the existing one.

Remember: the agent_id was returned when you first spawned the agent. You can also find
it by calling `agent_control(action: 'status', agent_id: '<id>')`.

After continuing the agent, tell the OPERATOR you've kicked off additional research with
their new criteria.

## Research Workflow (for non-agent research)

For the subset of research tasks that don't need a full agent (simple lookups, quick
facts, questions where 1-2 sources suffice):

1. **knowledge_search** — check existing notes, references, and diary entries first.
2. **web_search** — 1-2 targeted searches.
3. **web_fetch** — read at least 1-2 results in full. Never answer from snippets alone.
4. **Synthesize and respond** — cite sources you actually read.

## Source Evaluation

Authority hierarchy (highest to lowest):

1. **Official documentation** — manufacturer specs, API docs, project docs
2. **Dedicated review sites** — rtings.com, all3dp.com, wirecutter.com, dpreview.com,
   tomshardware.com, techpowerup.com
3. **Community consensus** — reddit threads with multiple upvoted responses, Stack
   Overflow accepted answers, HN discussions
4. **Specialized blogs** — individual experts with demonstrated domain knowledge
5. **General blogs and news** — useful for news, poor for recommendations
6. **AI-generated summaries** — treat as unreliable; always verify claims

When you find a high-quality source for a domain, note it. If rtings.com has definitive
test data for a product category, that outweighs ten blog posts.

## Citation Discipline

- Cite **2-3 authoritative sources**, not 8 shallow ones.
- Only cite sources you actually **read** (web_fetch'd). Search snippets are not
  citations.
- Include the URL and a brief description of what the source contains.
- If sources contradict each other, note the contradiction explicitly.

## Web Fetch Modes

- **`--readability`**: Best for articles, blog posts, reviews. Strips navigation and
  sidebars, gives you the article body.
- **Default** (no flags): Best for documentation, index pages, forums, search results.
  Preserves full page structure.
- **`--max-chars <N>`**: Use for very large pages. `--max-chars 30000` for long docs,
  `--max-chars 10000` for quick overview.

## Query Crafting

- **Be specific**: `"enclosed 3d printer under $1000 2024 review"` not `"3d printer"`
- **Include versions/years**: `"react 19 server components"` not
  `"react server components"`
- **Use quotes for exact phrases**: `"build volume" 3d printer enclosed`
- **Try different angles**: search for reviews, then comparisons, then community
  discussions. Each angle surfaces different sources.

## Crafting a Good Agent Prompt

When spawning `agent_control(action: 'start', agent: 'deep-research', prompt: '...')`,
include:

- **Specific question** — what exactly needs to be answered
- **Context** — budget, use case, constraints, preferences the OPERATOR mentioned
- **Scope** — what sub-questions to investigate
- **Quality bar** — how many sources, what kind of sources

Example:

```
Research enclosed 3D printers for home use. Budget $500-$1500.
Key questions: print quality, noise level, enclosed build volume,
software ecosystem, community support. Focus on models released
in the last 12 months. Prioritize rtings, all3dp, and reddit
r/3dprinting consensus over blog posts.
```

After spawning the agent, respond to the OPERATOR immediately: tell them you've started
a background research task and you'll share the findings when it completes.
