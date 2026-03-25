---
title: Providers
description:
  LLM backends supported by GHOST — OpenRouter, Kimi Code, OpenAI OAuth, and Anthropic.
---

A provider is an LLM backend. GHOST supports multiple providers and lets you define
named model aliases.

## Available Providers

| Provider             | ID             | Auth                                      |
| -------------------- | -------------- | ----------------------------------------- |
| OpenRouter           | `openrouter`   | `OPENROUTER_API_KEY` env var              |
| Kimi Code            | `kimi_code`    | `KIMI_API_KEY` env var                    |
| OpenAI OAuth (Codex) | `openai_oauth` | `ghost auth codex`                        |
| Anthropic (OAuth)    | `anthropic`    | Claude Code credentials (see setup below) |

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

:::note `default` specifies which alias to use when none is specified. Each alias needs
`provider`, `model`, and `context_window`. You can optionally add `headers` for extra
HTTP headers. :::

## OpenRouter Provider Routing

OpenRouter routes requests across multiple upstream providers. Use `provider_routing` to
control which providers receive your requests — for example, to restrict to providers
that support prompt caching:

```toml title="~/.config/ghost/config.toml"
[models.primary]
provider = "openrouter"
model = "anthropic/claude-sonnet-4"
context_window = 200000
provider_routing = { only = ["anthropic", "openai", "google", "deepseek"] }
```

Available fields:

| Field                | Type       | Description                                         |
| -------------------- | ---------- | --------------------------------------------------- |
| `only`               | `string[]` | Whitelist: only route to these providers            |
| `ignore`             | `string[]` | Blacklist: never route to these providers           |
| `order`              | `string[]` | Preferred provider order (first = highest priority) |
| `allow_fallbacks`    | `bool`     | Fall back when preferred providers fail             |
| `require_parameters` | `bool`     | Only use providers supporting all request params    |

This maps directly to the OpenRouter
[provider preferences](https://openrouter.ai/docs/guides/routing/provider-selection)
request field. It is ignored by other providers.

## Model Chains (Fallback)

Model references can be a single alias or an ordered list. When configured as a list,
GHOST tries each model in order — if the first fails with a retryable error (rate limit,
server error, timeout), it automatically falls through to the next.

```toml title="~/.config/ghost/config.toml"
[models]
# Single alias (standard)
default = "primary"

# Or a chain with automatic fallback
default = ["primary", "fallback", "tertiary"]

[models.primary]
provider = "anthropic"
model = "claude-sonnet-4-6"
context_window = 1000000

[models.fallback]
provider = "openrouter"
model = "anthropic/claude-sonnet-4-6"
context_window = 200000

[models.tertiary]
provider = "openrouter"
model = "google/gemini-2.0-flash"
context_window = 128000
```

Permanent errors (authentication, model not found) stop the chain immediately — there is
no point trying a fallback for a credentials problem.

Each provider in the chain has its own circuit breaker (3 consecutive failures → skip
for 60 seconds), so known-bad models are skipped quickly.

Agents can also use chains:

```lua
return {
    name = "my-agent",
    model = {"primary", "fallback"},
    -- ...
}
```

## Anthropic Provider Setup

The Anthropic provider talks directly to the Anthropic Messages API using Claude Code's
OAuth credentials. This gives GHOST access to Claude Opus, Sonnet, and other Claude
models through your existing Claude Code subscription — no separate API key needed.

Be aware this is very much against Anthropic's ToS and they could decide to enforce
their rules and ban your account. There are no customer protection rules where they live
so they might think banning users for using the service they paid for is acceptable, and
they would be in their rights!

### 1. Install and authenticate Claude Code

Run Claude Code and authenticate:

```bash
# Civilized one-time run
nix run nixpkgs#claude-code --impure

# Barbaric global install
npm install -g @anthropic-ai/claude-code
claude
```

This creates `~/.claude/.credentials.json` with your OAuth tokens.

### 2. Add a model alias

```toml title="~/.config/ghost/config.toml"
[models.claude]
provider = "anthropic"
model = "claude-sonnet-4-6"
context_window = 1000000
```

Available models include `claude-sonnet-4-6`, `claude-opus-4-6`, and
`claude-haiku-4-5-20251001`. See
[Anthropic's model docs](https://docs.anthropic.com/en/docs/about-claude/models) for the
full list.

:::note

Token refresh is automatic. GHOST reads `~/.claude/.credentials.json`, refreshes expired
tokens, and writes updated credentials back with file locking to avoid races with Claude
Code. You can also set the `ANTHROPIC_OAUTH_TOKEN` env var to use a token directly (no
refresh).

:::

## OpenAI OAuth Setup

```bash
# Authenticate with OpenAI (browser-based OAuth flow)
ghost auth codex

# Check status
ghost auth status

# Revoke tokens
ghost auth revoke
```
