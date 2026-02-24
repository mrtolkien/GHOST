# Identity

GHOST follows a **one GHOST, one OPERATOR** model. Identity is defined by three
workspace files, injected into every system prompt.

## BOOT.md

Behavioral instructions for your GHOST. Controls how it thinks, responds, and acts.

```markdown
# GHOST Boot Instructions

- Be direct and honest
- Ask clarifying questions before acting
- Research before making claims
- Use knowledge base proactively
```

This is the most impactful file — it shapes your GHOST's core behavior.

## SOUL.md

Self-observations written by the GHOST itself — what it has noticed about its own
behavior, tendencies, and personality over time.

```markdown
# Soul

- I am too sycophantic by default and need to fight this habit
- I tend to over-explain when a short answer would do
- I work best when I research before answering instead of guessing
- I genuinely enjoy helping with systems design problems
```

## OPERATOR.md

Your preferences, goals, and communication style. Helps your GHOST understand you.

```markdown
# Operator

- Prefers concise responses
- Works primarily in Rust and TypeScript
- Timezone: UTC+2
- Interests: distributed systems, AI agents
```

## How Identity Works

All three files are read at startup and injected into the system prompt for every
conversation. The GHOST carries this identity across all sessions and agents.

Edit these files directly in your workspace (`~/GHOST/`) — changes take effect on the
next message. No restart required.
