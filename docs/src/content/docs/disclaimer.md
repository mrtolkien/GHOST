---
title: Project Status
description: Where GHOST is today and where it's going.
---

GHOST is **extremely early and experimental**. Things break and entire subsystems get
rewritten between releases. If you try it today, expect rough edges — and please
[open an issue](https://github.com/mrtolkien/GHOST/issues) when you hit one.

## What I'm doing right now

I'm daily-driving GHOST and focusing exclusively on the core feature set.

My priorities are, in order:

- **Chat** — fast, token-efficient conversations with compaction and session management.
- **Knowledge** — notes, references, diary, wiki-link graph, hybrid BM25 + embedding
  search.
- **Security** - The shell tool is full YOLO at the moment, which is a bit too YOLO even
  for me.
- **Coding** — GHOST HACK: a coding agent inspired by pi-mono. As of 2026-03-27 it is
  pretty bad honestly.
- **Communication** - I want there to be some way to let trusted GHOST instances
  communicate to share knowledge and workflows. This will be the last big feature before
  1.0. Not started as of 2026-03-27.
- **Extensibility** — Autonomous creation of Lua agents, skills, cron jobs.

Everything else is secondary until these are solid.

## Provider integration quality

At the moment only OpenRouter is a "clean" provider integration. While OpenAI Codex,
Kimi Code, and Claude Code are available, they are against the ToS of those providers
and regularly have issues.

If an inference provider somedays offers both a flat monthly cost, decent limits, _and_
proper API access, please contact me and I'll add it. I have not found a provider
working that way at the moment.

## What's not in scope yet

More interfaces (web app first, then Slack, Telegram, ...), more LLM providers, plugin
systems, multi-user setups — all of that is on the roadmap but **none of it is being
actively worked on**. I don't have the bandwidth to review and validate contributions in
those areas while the foundation is still moving, so PRs adding new interfaces or
providers will likely sit unmerged for a while.

## When 1.0

Once I'm happy with daily-driving GHOST across all five core areas above, I'll release
v1.0 and start expanding the goals again.
