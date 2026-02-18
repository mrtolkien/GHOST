---
name: research
description:
  Research strategy, source evaluation, and citation discipline. Load before
  any research task.
triggers:
  - research
  - look up
  - search
  - find out
  - recommend
  - compare
  - best
---

# Research Skill

## Research Workflow

Follow this priority order for ANY research task:

1. **knowledge_search** — check existing notes, references, and diary entries first. You
   may already have authoritative information saved.
2. **web_search** — 2-3 searches with different angles to identify key sources.
3. **web_fetch** — ALWAYS read at least 2-3 results in full. Never answer
   product/recommendation questions from search snippets alone. Snippets are SEO bait;
   the real content is on the page.
4. **Synthesize and respond** — cite sources you actually read (web_fetch'd), not search
   snippets.

**Rule**: If you haven't web_fetch'd at least 2 sources, you haven't researched.

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

## Deep Research Escalation

For complex research tasks requiring:

- Multiple full page reads (5+)
- Comparative analysis across many sources
- Cross-referencing claims
- Product/technology recommendations with high stakes

Use `agent_control(action: 'start', agent: 'deep-research', prompt: '...')`.

### Crafting a Good Agent Prompt

Include in your prompt:

- **Specific question** — what exactly needs to be answered
- **Context** — budget, use case, constraints, preferences
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
