# Providers

A provider is an LLM backend. GHOST supports multiple providers and lets you define
named model aliases.

## Available Providers

| Provider             | ID             | Auth                         |
| -------------------- | -------------- | ---------------------------- |
| OpenRouter           | `openrouter`   | `OPENROUTER_API_KEY` env var |
| Kimi Code            | `kimi_code`    | `KIMI_API_KEY` env var       |
| OpenAI OAuth (Codex) | `openai_oauth` | `ghost auth codex`           |

## Model Aliases

Define aliases in `config.toml` to name your models:

```toml
[models]
default = "primary"

[models.primary]
provider = "openrouter"
model = "anthropic/claude-sonnet-4"
context_window = 200000

[models.fast]
provider = "kimi_code"
model = "kimi-k2.5"
context_window = 250000
```

- `default` — which alias to use when none is specified
- `provider` — one of: `openrouter`, `kimi_code`, `openai_oauth`
- `model` — the model ID as the provider expects it
- `context_window` — max tokens (used for compaction decisions)
- `headers` (optional) — extra HTTP headers for the provider

## Multiple Models

Different features can use different models:

- **Chat sessions** use the default model
- **Jobs** specify their own model alias in frontmatter
- **Agents** can override the model in their definition

## OpenAI OAuth Setup

```bash
# Authenticate with OpenAI (browser-based OAuth flow)
ghost auth codex

# Check status
ghost auth status

# Revoke tokens
ghost auth revoke
```
