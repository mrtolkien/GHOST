<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/public/logo-light.svg">
    <source media="(prefers-color-scheme: light)" srcset="docs/public/logo-dark.svg">
    <img src="docs/public/logo-dark.svg" alt="GHOST logo" width="96" height="96">
  </picture>
</p>

<h1 align="center">GHOST</h1>

<p align="center">
  <strong>Personal AI agent platform. One GHOST, one OPERATOR.</strong>
</p>

<p align="center">
  <a href="https://ghost.tolki.dev"><img alt="Docs" src="https://img.shields.io/badge/docs-ghost.tolki.dev-blue"></a>
  <a href="https://github.com/mrtolkien/GHOST/releases/latest"><img alt="GitHub Release" src="https://img.shields.io/github/v/release/mrtolkien/GHOST"></a>
  <a href="https://github.com/mrtolkien/GHOST/blob/main/LICENSE"><img alt="License: MIT" src="https://img.shields.io/github/license/mrtolkien/GHOST"></a>
</p>

---

A single Rust binary that runs your own AI agent with persistent memory, background
jobs, and multi-interface communication.

> **Extremely early and experimental.** See the
> [project status](https://ghost.tolki.dev/disclaimer/) page before diving in.

## What it does

- **Plain text workspace** — identity, agents, skills, and knowledge are editable files
  in `~/GHOST/`. `git diff` your GHOST.
- **Knowledge graph** — notes, references, diary with `[[typed>wiki links]]`, hybrid
  BM25 + embedding search, and reflection agents that learn from idle conversations.
- **Lua agents** — background workers with cron/idle triggers, restricted tools, and
  their own system prompts. Ships with deep-research and chat-reflection.
- **GHOST HACK** — a coding agent that loads project context, reads code, asks
  questions, runs tests, and commits.
- **Token efficiency** — minimal tools, minimal prompt, minimal context. Why use many
  tokens when few do trick?

## Getting started

Full installation and setup instructions are in the
**[docs](https://ghost.tolki.dev/getting-started/installation/)**.

GHOST installs via Nix with a binary cache so you don't have to compile from source. The
docs walk you through prerequisites, onboarding, and configuring providers and
interfaces.

## Architecture

```
ghost daemon
├── Discord bot          (interface)
├── Job scheduler        (cron + heartbeat + reflection)
├── Chat orchestration   (tool loop, compaction, sessions)
├── Provider layer       (OpenRouter, Kimi, OpenAI OAuth)
├── SQLite + sqlite-vec  (embedded, knowledge + embeddings)
└── Workspace files      (identity, skills, agents, jobs)
```

## Similar projects

GHOST was born out of daily-driving [OpenClaw](https://github.com/openclaw/openclaw) and
wanting something different. If GHOST isn't your thing, check out:

- **[pi-mono](https://github.com/pi-mono/pi)** — the coding agent that inspired GHOST
  HACK.
- **[OpenClaw](https://github.com/openclaw/openclaw)** — the OG open-source AI agent
  platform.
- **[ZeroClaw](https://github.com/zeroclaw-labs/zeroclaw)** — clean Rust implementation
  with great provider and interface abstractions.

## License

[MIT](LICENSE)
