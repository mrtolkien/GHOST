# Per-Model / Per-Agent Reasoning Effort

## Status: Done

## Context

Reasoning effort was hardcoded to `"high"` in both provider build functions. This wasted
tokens and latency for calls that don't need heavy reasoning (compaction, heartbeat
jobs), and prevented agents from requesting higher/lower effort.

## Design

Reasoning effort is configurable at three layers with a cascade resolution:

```
ChatRequest.reasoning_effort  (per-request, e.g. compaction → "low")
  > TaskDefinition.reasoning_effort  (agent/job frontmatter)
    > ModelConfig.reasoning_effort  (config.toml per-model)
      > "medium"  (global default)
```

### Config

```toml
[models.primary]
provider = "openai_oauth"
model = "gpt-5.3-codex"
reasoning_effort = "high"
```

### Agent/job frontmatter

```yaml
---
name: deep-research
reasoning_effort: high
---
```

## Provider Mapping

| Our value | OpenAI Codex                 | OpenRouter                   |
| --------- | ---------------------------- | ---------------------------- |
| `none`    | `reasoning.effort: "none"`   | `reasoning_effort: "none"`   |
| `low`     | `reasoning.effort: "low"`    | `reasoning_effort: "low"`    |
| `medium`  | `reasoning.effort: "medium"` | `reasoning_effort: "medium"` |
| `high`    | `reasoning.effort: "high"`   | `reasoning_effort: "high"`   |

OpenRouter handles cross-provider mapping automatically.

## Implementation

- `ReasoningEffort` enum (`None`/`Low`/`Medium`/`High`) in `src/providers/types.rs`
- `resolve_reasoning_effort()` implements the three-layer cascade
- `reasoning_effort: Option<ReasoningEffort>` on `ChatRequest`, `ModelConfig`,
  `TaskDefinition`, `JobDefinition`
- Providers read from `request.reasoning_effort` instead of hardcoding
- Compaction uses `Low`; deep-research agent uses `High`
- Default (when nothing is configured) is `Medium`
