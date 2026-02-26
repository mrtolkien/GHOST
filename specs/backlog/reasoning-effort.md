# Per-Model / Per-Agent Reasoning Effort

## Status: Backlog

## Context

We hardcode `reasoning_effort: "high"` for all provider calls (Codex Responses
API and OpenRouter/Kimi Chat Completions). This was added to fix a critical
issue: the model defaults to `effort: "none"` (zero chain-of-thought), causing
~20-30% empty responses per iteration in agentic loops. With `"high"`, the model
gets thinking tokens at each step.

However, not every call needs high reasoning:

- **Compaction** (summarizing conversation history) is mechanical — `"low"` or
  `"medium"` would save tokens and latency.
- **Simple chat** (single-turn Q&A) may not need heavy reasoning.
- **Agents** (deep-research, future coding agent) benefit from `"high"` or
  `"xhigh"` since they make complex multi-step decisions.
- **Heartbeat/reflection** jobs are lightweight — `"low"` suffices.

## Design

### Option A: Per-model config in `config.toml`

```toml
[models.aliases.fast]
provider = "openai_oauth"
model = "gpt-5.3-codex"
reasoning_effort = "high"

[models.aliases.cheap]
provider = "openrouter"
model = "moonshotai/kimi-k2.5"
reasoning_effort = "low"
```

### Option B: Per-agent in frontmatter

```yaml
---
name: deep-research
reasoning_effort: high
---
```

### Option C: Both (agent overrides model default)

Agent frontmatter > model config > global default (`"high"`).

## Provider Mapping

| Our value | OpenAI Codex | OpenRouter | Anthropic | Gemini 3.x |
|-----------|-------------|------------|-----------|------------|
| `none` | `reasoning.effort: "none"` | `reasoning_effort: "none"` | `effort: "low"` + `thinking: disabled` | `thinkingLevel: "minimal"` |
| `low` | `reasoning.effort: "low"` | `reasoning_effort: "low"` | `effort: "low"` | `thinkingLevel: "low"` |
| `medium` | `reasoning.effort: "medium"` | `reasoning_effort: "medium"` | `effort: "medium"` | `thinkingLevel: "medium"` |
| `high` | `reasoning.effort: "high"` | `reasoning_effort: "high"` | `effort: "high"` | `thinkingLevel: "high"` |

OpenRouter handles this mapping automatically when using `reasoning_effort`.

## Implementation

1. Add `reasoning_effort: Option<String>` to `ModelSettings` / `ModelConfig`.
2. Add `reasoning_effort: Option<String>` to agent `TaskFrontmatter`.
3. Add `reasoning_effort: Option<String>` to `ChatRequest`.
4. Resolution order: `ChatRequest` field (if set) > agent frontmatter > model
   config > `"high"` default.
5. Each provider maps to its native format (already done for Codex and
   OpenRouter).
6. Compaction calls should use `"low"`.
