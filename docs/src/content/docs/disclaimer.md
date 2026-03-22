---
title: Project Status
description: Where GHOST is today and where it's going.
---

GHOST is **extremely early and experimental**. Things break and entire subsystems get
rewritten between releases. If you try it today, expect rough edges — and please
[open an issue](https://github.com/mrtolkien/GHOST/issues) when you hit one.

## What I'm doing right now

I'm daily-driving GHOST and focusing exclusively on the core feature set:

- **Chat** — fast, token-efficient conversations with compaction and session management.
- **Knowledge** — notes, references, diary, wiki-link graph, hybrid BM25 + embedding
  search.
- **Coding** — GHOST HACK: a coding agent that loads project context, reads code, asks
  questions, runs tests, and commits.
- **Extensibility** — Lua agents, skills as plain text files, cron jobs.
- **Automation** — background agents for deep research, reflection, and proactive work.

Everything else is secondary until these are solid.

## What's not in scope yet

More interfaces (Telegram, Slack, a web app), more LLM providers, plugin systems,
multi-user setups — all of that is on the roadmap but **none of it is being actively
worked on**. I don't have the bandwidth to review and validate contributions in those
areas while the foundation is still moving, so PRs adding new interfaces or providers
will likely sit unmerged for a while.

## When 1.0

Once I'm happy with daily-driving GHOST across all five core areas above, I'll release
v1.0 and start adding those secondary features and merging PRs related to them.
