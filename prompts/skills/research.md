---
name: research
description:
  MANDATORY for any question involving recommendations, comparisons, buying decisions,
  evaluations, or current web data. You MUST call
  read_file("skills/research/skill.md") BEFORE generating any response to these
  questions. Do NOT answer from training data — it produces dangerously outdated
  advice. This skill contains non-negotiable workflow rules including when to use
  background agents for multi-source research vs direct tools for simple lookups.
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

1. **Domain specialists** — sites dedicated to ONE field (e.g. a site that only covers
   3D printing, or only audio equipment) with testing methodology and measurements.
   These beat generalist tech sites because depth of expertise matters more than brand
   recognition. The agent discovers these via community threads during research.
2. **Individual expert reviewers** — people known in the community for rigorous testing
   in this specific domain (YouTube channels, personal sites, blogs)
3. **Official documentation** — manufacturer specs, API docs, project docs
4. **Community consensus** — reddit threads with multiple upvoted responses, Stack
   Overflow accepted answers, HN discussions. Also the best way to discover tier-1 and
   tier-2 sources.
5. **Generalist tech publications** — sites that review many categories. Useful as
   supplements but not primary sources when domain specialists exist.
6. **SEO "best of" listicles** — usually AI-generated or affiliate-driven. Treat as
   unreliable unless the site demonstrably does actual testing.

What makes a source authoritative is NOT its name or SEO ranking but its domain focus
and methodology: a site that ONLY covers one field and tests rigorously will always
outperform a generalist that reviews everything.

The GHOST's knowledge base accumulates source quality notes over time through
reflection. When spawning a research agent, this prior knowledge helps it skip the
source-discovery phase and go straight to trusted sources.

## Citation Discipline

- Cite **2-3 authoritative sources**, not 8 shallow ones.
- Only cite sources you actually **read** (web_fetch'd). Search snippets are not
  citations.
- Include the URL and a brief description of what the source contains.
- If sources contradict each other, note the contradiction explicitly.

## Web Fetch Modes

- **`readability: true`**: Best for articles, blog posts, reviews. Strips navigation and
  sidebars, gives you the article body.
- **Default (no readability)**: Best for documentation, index pages, forums, product
  listings, and price trackers. Preserves full page structure — important when you need
  to see all items on the page.
- **Do NOT use readability for product listings, price trackers, or comparison tables**
  — you need the full page to see every item listed.

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
- **Recency** — remind the agent to look for recently released models/versions/products
  the OPERATOR might not know about. The agent's training data is outdated.
- **Quality bar** — how many sources, what kind of sources The agent will automatically
  discover authoritative sources for the domain before searching for answers. If past
  reflection has recorded trusted sources for this topic, mention them in the prompt —
  but the agent validates independently.

Example:

```
Research enclosed 3D printers for home use. Budget $500-$1500.
Key questions: print quality, noise level, enclosed build volume,
software ecosystem, community support. Focus on models currently
available. Find which review sites the community trusts for 3D
printer testing, then use those sources.
```

After spawning the agent, respond to the OPERATOR immediately: tell them you've started
a background research task and you'll share the findings when it completes.
