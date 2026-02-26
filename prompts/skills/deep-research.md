---
name: deep-research
description:
  Read before starting a complex research task that requires discovering authoritative
  sources, reading 5+ full pages, and cross-referencing claims across multiple sources.
  Examples: product comparisons, technology evaluations, market analysis, multi-factor
  decisions. Do NOT read for simple lookups, quick facts, or questions answerable with
  your knowledge base and a few web searches.
---

# Deep Research Skill

You're reading this because the OPERATOR's question looks like it might need deep,
multi-source research. This skill helps you decide whether to spawn a `deep-research`
agent or handle it yourself.

## Decision: Agent or Not?

Spawning the deep-research agent is expensive — it runs autonomously for several
minutes, reads many pages, and uses significant context. Only spawn it when the research
genuinely requires that depth.

### Spawn the agent when ALL of these are true:

1. **Your knowledge base has no good answer** — you already checked `knowledge_search`
   and found nothing relevant or only outdated information.
2. **Multiple sources need cross-referencing** — the question requires comparing claims
   across 5+ independent sources to form a reliable answer.
3. **Source discovery is needed** — you don't already know which sources to trust for
   this domain. The agent's strength is finding authoritative sources from scratch by
   checking what the community recommends.
4. **The answer can't be assembled in 2-3 search+fetch cycles** — if you can get a solid
   answer by searching, reading 1-2 pages, and synthesizing, do it yourself.

### Do NOT spawn the agent for:

- Questions where your knowledge base already has good notes or references
- Questions answerable from 1-3 web fetches (even if the topic is complex)
- Questions where you already know the authoritative sources — just fetch them directly
- Follow-up questions on a topic you recently researched (check knowledge first)
- Simple factual lookups, definitions, or explanations

### When in doubt:

Try answering it yourself first with `knowledge_search` + `web_search` + `web_fetch`. If
after 2-3 searches you realize the topic is deeper than expected and you need to
discover sources from scratch, THEN spawn the agent. Starting small costs less than
starting big.

## Spawning the Agent

When you decide to spawn, use
`agent_control(action: 'start', agent: 'deep-research', prompt: '...')`. Include:

- **Specific question** — what exactly needs to be answered
- **Context** — constraints, preferences, use case the OPERATOR mentioned
- **Scope** — what sub-questions to investigate
- **Recency** — remind the agent to look for recent developments
- **Known sources** — if your knowledge base has source quality notes for this domain,
  pass them to the agent so it can skip the source-discovery phase

After spawning, tell the OPERATOR you've started a background research task and you'll
share findings when it completes.

## Follow-Up Questions

When the OPERATOR provides follow-up criteria or refinements after the agent delivered
findings, **continue** the existing agent session:

```
agent_control(action: 'continue', agent_id: '<id>',
  prompt: '<new constraints and follow-up question>')
```

The agent already has full context from its research. Continuing lets it do targeted
follow-up searches instead of starting from scratch. Never spawn a new agent for
follow-ups on the same topic.
