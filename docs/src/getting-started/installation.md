# Installation

## Prerequisites

- **Rust toolchain** — [rustup.rs](https://rustup.rs/)
- **Ollama** — for local embeddings ([ollama.com](https://ollama.com))
- **Crawl4AI** — for web page extraction
  ([crawl4ai](https://github.com/unclecode/crawl4ai))
- **Discord bot token** — for the Discord interface (see
  [Interfaces](../ghost/interfaces.md))

> In the future, all dependencies beyond the binary itself will be managed by the GHOST
> through Nix. For now, you need to set them up manually.

## Install

```bash
# Clone and install
git clone https://github.com/tolki/ghost.git
cd ghost
cargo install --path .
```

## Bootstrap

```bash
# Initialize workspace and config
ghost init
```

This creates:

- Config file at `~/.config/ghost/config.toml`
- Workspace directory at `~/GHOST/` with default identity files, skills, and agents

## Secrets

Create a `.env` file (or set environment variables). **Never put API keys in
`config.toml`.**

```bash
# Required for at least one provider
OPENROUTER_API_KEY=sk-or-...

# Required for web search
BRAVE_API_KEY=BSA...

# Required for Discord
DISCORD_TOKEN=...
```

## Pull Embedding Model

```bash
ollama pull qwen3-embedding:8b
```

## Start Crawl4AI

GHOST uses a Crawl4AI container for web page extraction. Run it with Docker:

```bash
docker run -d -p 11235:11235 unclecode/crawl4ai
```

Then set the URL in `config.toml`:

```toml
[web]
crawl4ai_url = "http://localhost:11235"
```

## First Run

```bash
ghost daemon
```

This starts the Discord bot and the job scheduler. Your GHOST is now live.
