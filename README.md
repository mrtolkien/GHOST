# GHOST

> Personal AI agent platform. One GHOST, one OPERATOR.

A single binary that runs your own AI companion with persistent memory, background jobs,
and multi-interface communication.

## Features

- **Persistent knowledge** — notes, references, and diary with graph edges and semantic
  search (SQLite + sqlite-vec + Ollama embeddings)
- **Background agents** — autonomous workers for research, reflection, and proactive
  check-ins
- **Cron jobs** — scheduled tasks defined as markdown files
- **Skills** — teachable workflows your GHOST learns from text files
- **Identity** — configurable personality, values, and behavioral instructions
- **Multiple LLM providers** — OpenRouter, Kimi Code, OpenAI OAuth (Codex)
- **Discord interface** — chat with your GHOST in Discord threads

## Quick Start

```bash
# Install
cargo install --path .

# Bootstrap workspace and config
ghost init

# Pull embedding model
ollama pull qwen3-embedding:8b

# Set secrets
export OPENROUTER_API_KEY=sk-or-...
export DISCORD_TOKEN=...
export BRAVE_API_KEY=BSA...

# Run
ghost daemon
```

## Documentation

Full docs: **[ghost docs](https://tolki.github.io/ghost/)** (or `mdbook serve docs/`
locally)

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

## License

MIT
