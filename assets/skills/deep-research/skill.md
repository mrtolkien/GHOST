---
name: deep-research
description:
  Read when the OPERATOR asks a question that will require web research across multiple
  sources — recommendations, comparisons, evaluations, multi-factor decisions, "what
  should I buy/use", or any question where you'd need to read several web pages. This
  skill decides whether to spawn a background research agent (to protect your context
  from heavy fetching) or handle it yourself. Do NOT read for simple factual lookups or
  questions fully answered by your knowledge base.
---

# Deep Research Skill

You're reading this because the OPERATOR's question needs multi-source research.

## Why the Agent Exists

Each `web_fetch` dumps thousands of tokens into your context. Doing several fetches
inline to answer one question pollutes your main conversation — past messages get
compressed, future turns get worse. The deep-research agent runs in an **isolated
context** that is discarded after it delivers a summary. It protects your conversation
while doing the heavy reading.

## Decision Process

### Step 1: Check knowledge

Call `knowledge_search` first. If you find existing notes or references that adequately
answer the question, use them and respond directly. No agent needed.

### Step 1.5: Consider reference import first

If the research centers on a **specific tool, framework, or library** (e.g. Ergogen,
Dioxus, KiCad), check whether its official docs can be imported via the
`reference-import` skill. Imported docs persist in your knowledge base and are
searchable across sessions — far more valuable than ephemeral agent context. Read the
reference-import skill, start the import in the background, and _then_ optionally spawn
the deep-research agent for the broader question (e.g. community guides, best practices)
while the import runs.

### Step 2: Spawn the agent

If knowledge didn't have a good answer (and reference import doesn't apply or is already
running), spawn the deep-research agent. You matched this skill's description because
the question needs multi-source research — that research belongs in the agent's isolated
context, not inline.

**Your next tool call after the knowledge check must be `agent`.** Do not call
`web_search` or `web_fetch` — every page you fetch inline is context you can never
reclaim. Let the agent do the heavy reading.

## Spawning the Agent

Use `agent(action: 'start', name: 'deep-research', prompt: '...')`. Include:

- **Specific question** — what exactly needs to be answered
- **Context** — constraints, preferences, use case the OPERATOR mentioned
- **Scope** — what sub-questions to investigate
- **Recency** — remind the agent to look for recent developments
- **Known sources** — if your knowledge base has source quality notes for this domain,
  pass them to the agent so it can skip the source-discovery phase
- **Don't name specific options** — describe the category, constraints, and use case,
  but do NOT pre-populate specific names, brands, or recommendations from your training
  data. Your knowledge is outdated — let the agent discover the current landscape
  through research. Bad: "looking at X, Y, and Z". Good: "looking for a [category] that
  meets [constraints]".

After spawning, tell the OPERATOR you've started a background research task and you'll
share findings when it completes.

## Follow-Up Questions

When the OPERATOR provides follow-up criteria or refinements after the agent delivered
findings, **continue** the existing agent session:

```
agent(action: 'continue', agent_id: '<id>',
  prompt: '<new constraints and follow-up question>')
```

The agent already has full context from its research. Continuing lets it do targeted
follow-up searches instead of starting from scratch. Never spawn a new agent for
follow-ups on the same topic.
