# Backlog — Additional Providers

## Overview

Add direct provider adapters beyond OpenRouter for:

- Better pricing (no OpenRouter markup)
- Provider-specific features (Anthropic prompt caching, Gemini long context)
- Resilience (direct fallback when OpenRouter is down)

## Inspiration

Pi mono:

https://github.com/badlogic/pi-mono/tree/main/packages/ai

ZeroClaw:

https://github.com/zeroclaw-labs/zeroclaw/tree/master/src/providers

## Planned Providers

### Claude Code

Also called Anthropic OAUTH sometimes.

Against ToS, but they don't really care it seems

### Google Gemini

- Has a free tier
- API: `https://generativelanguage.googleapis.com/v1beta/`

### Anthropic (Direct)

- Native Claude API with prompt caching support
- Extended thinking for complex reasoning
- Better rate limits than through OpenRouter
- API: `https://api.anthropic.com/v1/messages`

### OpenAI-Compatible (Generic)

- Support local models (llama.cpp, vLLM, Ollama chat)
- Configurable base URL
- Useful for private/sensitive workloads

## Model Chains

With multiple providers, re-enable model chain fallback.

Check the model chain note too, next to this one.

```toml
default_model = ["primary", "fallback", "tertiary"]

[models.primary]
provider = "anthropic"
model = "claude-sonnet-4-5-20250929"

[models.fallback]
provider = "openrouter"
model = "anthropic/claude-sonnet-4-5-20250929"

[models.tertiary]
provider = "gemini"
model = "gemini-2.0-flash"
```

Maybe even something like:

```toml
[model.main]
provider = ["primary", "fallback", "tertiary"]
```

Chain resolution: try primary, if rate limited try the next one, and so on and so forth.
Circuit breaker prevents hammering rate-limited providers.

## Implementation

Each provider implements the existing `Provider` trait. See `05-providers.md` for the
trait definition. The `docs/dev/add-provider.md` guide from t-koma provides a detailed
checklist.
