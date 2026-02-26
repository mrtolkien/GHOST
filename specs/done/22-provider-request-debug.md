# Spec 22: Provider Request Debug Logging

## Problem

When a live test fails or the GHOST behaves unexpectedly, the first debugging step is
always "what did we actually send to the provider?" Currently there's no way to see the
raw request/response JSON without adding ad-hoc logging. The codex responses parser has
53+ SSE event types, the OpenAI-compatible path has its own message conversion, and each
provider serializes differently. Bugs in any of these are invisible without the raw
JSON.

This is a **provider trait concern**, not a test concern. The debug output must work for
all providers, at runtime, not just in tests.

## Design

### Config

```toml
[debug]
save_requests = true # default: false
```

When enabled, every `Provider::chat()` call saves its raw request and response to the
workspace debug directory.

### Output Location

```
$WORKSPACE/debug/requests/<timestamp>_<session_id_short>_<iteration>.json
```

Each file contains:

```json
{
  "timestamp": "2026-02-21T06:33:09Z",
  "provider": "openrouter",
  "model": "anthropic/claude-sonnet-4-5-20250929",
  "session_id": "session:abc123",
  "iteration": 3,
  "request": { ... },
  "response": { ... }
}
```

- `request`: the **provider-specific** serialized body — what actually goes over the
  wire. For OpenAI-compatible providers this is the `ChatCompletionsRequest` JSON. For
  codex responses, it's the codex request body.
- `response`: the raw HTTP response body (string), before parsing.

### Implementation

The debug saving happens **inside each provider's `chat()` implementation** (or in a
shared wrapper), not in the tool loop. This ensures:

1. Every provider is covered (OpenRouter, Kimi, OpenAI OAuth)
2. The saved request is the actual wire format, not our internal `ChatRequest`
3. Retries, error responses, and edge cases are all captured

#### Approach: Wrapper method in `OpenAiCompatibleProvider`

`send_request` already has the serialized body and raw response. Add a
`save_debug_request` helper that writes the file when `debug.save_requests` is true.
Config needs to be threaded through to the provider — either via a new field on the
provider struct or via a `DebugConfig` parameter on `chat()`.

For `OpenAiOAuthProvider`, the same pattern applies to its `send_request`.

#### Config threading

Options (decide during implementation):

1. **Provider constructor receives `debug.save_requests` + workspace path** — simplest,
   providers already receive config-derived values
2. **Shared `DebugRequestLogger` struct** passed to providers — more testable

### What gets saved

- The full serialized request body (JSON)
- The full raw response body (string, before parsing)
- Provider name, model, timestamp, session ID, iteration number
- HTTP status code
- Duration in ms

### Cleanup

Files in `debug/requests/` are ephemeral. The GHOST does not auto-clean them. The
OPERATOR manages this directory manually or via a cron job.

## Integration with Tests

Live tests (`LiveTestEnv`) should enable `debug.save_requests = true` by default. The
debug output directory is inside the temp workspace, so it gets snapshotted to
`e2e-output/` on drop alongside the diagnostic JSON.

This replaces ad-hoc diagnostic logging in individual tests — instead of each test
extracting messages from the DB and formatting them, the raw provider requests are
always available in the debug directory.

## Files

| File                                          | Change                                        |
| --------------------------------------------- | --------------------------------------------- |
| `src/config.rs`                               | Add `debug.save_requests` config field        |
| `src/providers/openai_compatible_provider.rs` | Save request/response when enabled            |
| `src/providers/openai_oauth.rs`               | Save request/response when enabled            |
| `tests/common.rs`                             | Enable `debug.save_requests` in `LiveTestEnv` |

## Non-Goals

- Streaming/SSE event-level logging (codex responses stream) — only the final parsed
  response is saved, not individual SSE events
- Request replay tooling — just saving, not replaying
- Automatic request diffing between runs
