# Introduction

GHOST is a personal AI agent platform. A single binary runs one GHOST for one OPERATOR —
your own AI companion with persistent memory, background jobs, and multi-interface
communication.

## Philosophy

- **Text-first** — workspace files (markdown, TOML) are the primary feature surface.
  Identity, skills, jobs, and knowledge all live as editable files.
- **CLI-first** — every feature is accessible through direct commands.
- **Skills over tools** — prefer reusable workflow files over adding new tool APIs.
- **Local-first** — embedded database, local embeddings, your data stays on your
  machine.

## Architecture

```
┌──────────────────────────────────────────┐
│               ghost daemon               │
├──────────┬───────────┬───────────────────┤
│ Discord  │ Job       │ Chat              │
│ Bot      │ Scheduler │ Orchestration     │
├──────────┴───────────┴───────────────────┤
│ Provider Layer (OpenRouter, Kimi, Codex) │
├──────────────────────────────────────────┤
│ SurrealDB (embedded)  │  Workspace Files │
└──────────────────────────────────────────┘
```

- **Single binary** — no external services required (except Ollama for embeddings)
- **Embedded SurrealDB** — graph-based knowledge storage with typed edges
- **Discord transport** — primary interface for the PoC (more planned)
- **Multiple LLM providers** — OpenRouter, Kimi Code, OpenAI OAuth (Codex)
