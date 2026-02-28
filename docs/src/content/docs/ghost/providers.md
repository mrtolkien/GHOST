---
title: Providers
description: LLM backends supported by GHOST — OpenRouter, Kimi Code, and OpenAI OAuth.
---

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

```toml title="~/.config/ghost/config.toml"
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

:::note
`default` specifies which alias to use when none is specified. Each alias
needs `provider`, `model`, and `context_window`. You can optionally add
`headers` for extra HTTP headers.
:::

## OpenRouter Provider Routing

OpenRouter routes requests across multiple upstream providers. Use
`provider_routing` to control which providers receive your requests — for
example, to restrict to providers that support prompt caching:

```toml title="~/.config/ghost/config.toml"
[models.primary]
provider = "openrouter"
model = "anthropic/claude-sonnet-4"
context_window = 200000
provider_routing = { only = ["anthropic", "openai", "google", "deepseek"] }
```

Available fields:

| Field                | Type       | Description                                       |
| -------------------- | ---------- | ------------------------------------------------- |
| `only`               | `string[]` | Whitelist: only route to these providers           |
| `ignore`             | `string[]` | Blacklist: never route to these providers          |
| `order`              | `string[]` | Preferred provider order (first = highest priority)|
| `allow_fallbacks`    | `bool`     | Fall back when preferred providers fail            |
| `require_parameters` | `bool`     | Only use providers supporting all request params   |

This maps directly to the OpenRouter
[provider preferences](https://openrouter.ai/docs/guides/routing/provider-selection)
request field. It is ignored by other providers.

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
