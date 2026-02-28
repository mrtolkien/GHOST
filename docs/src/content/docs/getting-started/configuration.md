---
title: Configuration
description: GHOST config file format, CLI access, and secrets management.
---

## Config File

Location: `~/.config/ghost/config.toml`

Override with `GHOST_CONFIG_DIR` environment variable.

## Full Example

```toml title="~/.config/ghost/config.toml"
# Workspace path (default: ~/GHOST)
workspace = "~/GHOST"

# Model aliases — define one or more (keys are flattened under [models])
[models]
default = "primary" # Which alias to use by default

[models.primary]
provider = "openrouter"
model = "anthropic/claude-sonnet-4"
context_window = 200000
provider_routing = { only = ["anthropic", "openai", "google"] }

[models.fast]
provider = "kimi_code"
model = "kimi-k2.5"
context_window = 250000

[models.codex]
provider = "openai_oauth"
model = "gpt-5.3-codex"
context_window = 250000

# Discord
[discord]
enabled = true
allowed_user_id = "123456789012345678" # Your Discord user ID

# Embeddings (Ollama)
[embeddings]
url = "http://127.0.0.1:11434" # Ollama endpoint
model = "qwen3-embedding:8b" # Embedding model
batch_size = 32 # Vectors per batch
dimension = 1024 # Vector dimension

# Timing
[timing]
heartbeat_idle_minutes = 4 # Minutes idle before heartbeat
heartbeat_check_seconds = 60 # How often to check for idle
heartbeat_continue_minutes = 30 # Max heartbeat conversation length
reflection_idle_minutes = 4 # Minutes idle before reflection runs
scheduler_tick_seconds = 10 # Cron scheduler poll interval

# Compaction (context window management)
[compaction]
threshold = 0.85 # Compact when context is 85% full
keep_window = 20 # Messages to keep after compaction
mask_preview_chars = 100 # Preview chars for compacted messages

# Web
[web]
search_max_results = 5 # Default Brave search results
# crawl4ai_url = "http://localhost:11235"  # Optional Crawl4AI backend

# Debug
[debug]
save_requests = false # Save raw provider requests to disk
```

## CLI Access

```bash
# Read a config value
ghost config get discord.allowed_user_id

# Set a config value
ghost config set timing.heartbeat_idle_minutes 10
```

## Secrets

:::danger[Security]
Never put API keys in `config.toml`. Use environment variables or a `.env`
file.
:::

| Variable             | Purpose             |
| -------------------- | ------------------- |
| `OPENROUTER_API_KEY` | OpenRouter provider |
| `BRAVE_API_KEY`      | Web search          |
| `DISCORD_TOKEN`      | Discord bot         |
| `KIMI_API_KEY`       | Kimi Code provider  |
