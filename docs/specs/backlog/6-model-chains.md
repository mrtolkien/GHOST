# Backlog — Model Chain Fallback

## Overview

Allow configuring ordered lists of model aliases as fallback chains. When the primary
model fails (rate limit, error), automatically try the next model in the chain.

## Config

```toml
default_model = ["primary", "fallback", "tertiary"]
heartbeat_model = ["fast", "primary"]

[models.primary]
provider = "anthropic"
model = "claude-sonnet-4-5-20250929"

[models.fallback]
provider = "openrouter"
model = "anthropic/claude-sonnet-4-5-20250929"

[models.tertiary]
provider = "gemini"
model = "gemini-2.0-flash"

[models.fast]
provider = "openrouter"
model = "google/gemini-2.0-flash-001"
```

## Circuit Breaker

Per-model circuit breaker:

- Track consecutive failures per model alias
- After 3 failures, mark as "open" (skip) for 60 seconds
- Log clearly when circuit opens/closes
- Useful for detecting and avoiding rate-limited models

## Implementation

This requires the provider trait and multiple provider implementations (see
`additional-providers.md`). The orchestration layer tries each model in order until one
succeeds.

## Dependencies

- At least two provider implementations
- Circuit breaker module
