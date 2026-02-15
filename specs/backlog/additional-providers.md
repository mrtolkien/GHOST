# Backlog — Additional Providers

## Overview

Add direct provider adapters beyond OpenRouter for:

- Better pricing (no OpenRouter markup)
- Provider-specific features (Anthropic prompt caching, Gemini long context)
- Resilience (direct fallback when OpenRouter is down)

## Planned Providers

### Anthropic (Direct)

- Native Claude API with prompt caching support
- Extended thinking for complex reasoning
- Better rate limits than through OpenRouter
- API: `https://api.anthropic.com/v1/messages`

### Google Gemini

- Very long context windows (1M+ tokens)
- Good for bulk analysis tasks
- API: `https://generativelanguage.googleapis.com/v1beta/`

### OpenAI-Compatible (Generic)

- Support local models (llama.cpp, vLLM, Ollama chat)
- Configurable base URL
- Useful for private/sensitive workloads

### Kimi Code (Moonshot AI)

- Specialized for code tasks
- Good pricing for code-heavy workloads

## Model Chains

With multiple providers, re-enable model chain fallback:

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

Chain resolution: try primary, if rate limited try fallback, then tertiary.
Circuit breaker prevents hammering rate-limited providers.

## Implementation

Each provider implements the existing `Provider` trait. See `05-providers.md` for the
trait definition. The `docs/dev/add-provider.md` guide from t-koma provides a detailed
checklist.
