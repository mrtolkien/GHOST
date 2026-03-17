# Anthropic OAuth Provider (Claude Code Credentials)

## Motivation

Add the Anthropic Messages API as a provider, authenticated via Claude Code's OAuth
tokens. This gives Ghost access to the full Claude model lineup (Opus, Sonnet, Haiku)
using the user's existing Claude Code subscription — no separate API key needed.

This is NOT the Claude Code CLI as a subprocess. It's a direct integration with the
Anthropic Messages API, using the same OAuth credentials that Claude Code stores locally.

## Prior Art: pi-mono

pi-mono (`badlogic/pi-mono`) implements exactly this pattern in
`packages/ai/src/providers/anthropic.ts` and `packages/ai/src/utils/oauth/anthropic.ts`.
Our implementation mirrors pi-mono's approach. Deviations are explicitly called out.

## Auth

### Credential Cascade

1. Env var `ANTHROPIC_OAUTH_TOKEN` (access token only, no refresh)
2. Auto-read from `~/.claude/.credentials.json` → `claudeAiOauth`

Detect OAuth tokens by the `sk-ant-oat` prefix (per pi-mono's `isOAuthToken()`).

### Credentials File Format

```json
{
  "claudeAiOauth": {
    "accessToken": "sk-ant-oat01-...",
    "refreshToken": "sk-ant-ort01-...",
    "expiresAt": 1773742487280,
    "scopes": ["user:inference", "..."],
    "subscriptionType": "max",
    "rateLimitTier": "default_claude_max_5x"
  }
}
```

- `expiresAt`: Unix timestamp in milliseconds
- Access tokens expire after ~8 hours
- Refresh tokens are single-use (rotated on each refresh)

### Token Refresh

Mirror pi-mono exactly:

1. Before each API call, check if `expiresAt - 5min < now`
2. If expired/expiring, POST to `https://platform.claude.com/v1/oauth/token`:
   ```json
   {
     "grant_type": "refresh_token",
     "client_id": "9d1c250a-e61b-44d9-88ed-5944d1962f5e",
     "refresh_token": "sk-ant-ort01-..."
   }
   ```
3. Response: `{ access_token, refresh_token, expires_in }`
4. Compute `expiresAt = now + expires_in * 1000 - 5min` (5-minute safety buffer, per
   pi-mono)
5. Write new credentials back to `~/.claude/.credentials.json`
6. Use a file lock (`fd-lock` or `fs2`) to avoid races with Claude Code or other
   Ghost processes
7. 30-second timeout on the refresh HTTP request

If the env var is used (no refresh token available), skip refresh and fail with a clear
error if the token is rejected.

## API Contract

### Endpoint

`POST https://api.anthropic.com/v1/messages`

### Headers

```
Authorization: Bearer sk-ant-oat-...
Content-Type: application/json
accept: application/json
anthropic-version: 2023-06-01
anthropic-beta: claude-code-20250219,oauth-2025-04-20,fine-grained-tool-streaming-2025-05-14
user-agent: claude-cli/2.1.75
x-app: cli
```

Note: `user-agent` version is static (matching pi-mono). Will go stale but unlikely to
matter — update periodically if needed.

Additionally, for older models (not Opus 4.6 / Sonnet 4.6), append
`interleaved-thinking-2025-05-14` to the `anthropic-beta` header. Per pi-mono: "The beta
header is deprecated on Opus 4.6 and redundant on Sonnet 4.6."

Model detection uses substring matching on model ID: `opus-4-6`, `opus-4.6`,
`sonnet-4-6`, `sonnet-4.6` (with or without date suffixes).

### System Prompt

OAuth requires the system prompt to start with a specific Claude Code preamble. The
provider silently prepends this before Ghost's actual system prompt:

```
You are Claude Code, Anthropic's official CLI for Claude.\n\n{ghost_system_prompt}
```

### Request Format

```json
{
  "model": "claude-sonnet-4-6-20250514",
  "max_tokens": 16384,
  "stream": true,
  "system": [
    {
      "type": "text",
      "text": "You are Claude Code...\n\n{ghost_system_prompt}",
      "cache_control": {"type": "ephemeral"}
    }
  ],
  "messages": [...],
  "tools": [...],
  "tool_choice": {"type": "auto"},
  "metadata": {"user_id": "..."}
}
```

- `stream: true` always explicit
- `temperature` is NOT sent when thinking is enabled (API rejects it)
- `metadata.user_id` included when available (from config or session context)

### Tool Name Translation (Claude Code Stealth Mode)

Per pi-mono: when using OAuth tokens, tool names must be translated to Claude Code's
canonical casing. This applies to tool definitions in requests, `tool_use` blocks in
assistant message history, and `tool_use` events during streaming.

Canonical names: `Read`, `Write`, `Edit`, `Bash`, `Grep`, `Glob`, `AskUserQuestion`,
`EnterPlanMode`, `ExitPlanMode`, `KillShell`, `NotebookEdit`, `Skill`, `Task`,
`TaskOutput`, `TodoWrite`, `WebFetch`, `WebSearch`.

- `toClaudeCodeName()`: case-insensitive match from Ghost tool name → canonical name
- `fromClaudeCodeName()`: reverse map from canonical name → Ghost's original tool name

Tool names that don't match any canonical name are passed through unchanged.

### Tool Call ID Normalization

Per pi-mono: strip non-`[a-zA-Z0-9_-]` characters (replace with `_`) and truncate to
64 chars. Handles cross-provider ID format differences (e.g., Codex Responses API
generates 450+ char IDs with `|` characters).

### Streaming (SSE)

Event flow: `message_start` → (`content_block_start` → `content_block_delta`* →
`content_block_stop`)* → `message_delta` → `message_stop`

**Delta types:**
- `text_delta` → accumulate into `ContentBlock::Text`
- `input_json_delta` → accumulate partial JSON for tool input
- `thinking_delta` → accumulate into thinking block
- `signature_delta` → accumulate signature for thinking block

**Terminal events:**
- `message_delta` carries `stop_reason` and final `usage`
- `message_stop` signals end of stream

**Usage handling:**
- Capture `input_tokens` from `message_start` immediately (available even if stream
  aborts)
- Update usage from `message_delta` only for non-null fields (preserves `input_tokens`
  from `message_start` when proxies omit it in `message_delta`)

**Content block index tracking:** Each block carries an `index` from the SSE events,
used to correlate deltas and stops to the correct block. Cleaned up on
`content_block_stop`.

**`redacted_thinking` in content_block_start:** Store `data` field as the thinking
signature, set content to `"[Reasoning redacted]"`.

### Response Mapping

| Anthropic | Ghost |
|---|---|
| `text` block | `ContentBlock::Text` |
| `tool_use` block | `ContentBlock::ToolUse` (reverse tool name translation) |
| `thinking` block | `ContentBlock::RawOutput` (preserve as-is for echo-back) |
| `redacted_thinking` block | `ContentBlock::RawOutput` (preserve `data` field) |
| `stop_reason: "end_turn"` | `StopReason::EndTurn` |
| `stop_reason: "tool_use"` | `StopReason::ToolUse` |
| `stop_reason: "max_tokens"` | `StopReason::MaxTokens` |
| `stop_reason: "pause_turn"` | `StopReason::ToolUse` (triggers continuation turn) |
| `stop_reason: "stop_sequence"` | `StopReason::EndTurn` |
| `stop_reason: "sensitive"` | `ProviderError::InvalidResponse` (safety filter) |
| `stop_reason: "refusal"` | `ProviderError::InvalidResponse` (refusal) |

Unknown stop reasons → `ProviderError` (don't silently default).

### Usage Mapping

```
input_tokens → usage.input_tokens
output_tokens → usage.output_tokens
cache_creation_input_tokens → usage.cache_creation_tokens
cache_read_input_tokens → usage.cache_read_tokens
```

### Tool Use

Anthropic native format — no nested `function` wrapper:

- Request: `tools` array with `name`, `description`, `input_schema`
- Response: `tool_use` content blocks with `id`, `name`, `input`
- Tool results: sent as user message with `tool_result` content blocks (including
  `is_error`)

### Thinking / Reasoning

**Adaptive (Opus 4.6 / Sonnet 4.6):**
- `thinking: {type: "adaptive"}`
- `output_config: {effort: "low"/"medium"/"high"/"max"}` (max only for Opus 4.6)

**Budget-based (older models):**
- `thinking: {type: "enabled", budget_tokens: N}`
- Budget by effort level (per pi-mono): minimal=1024, low=2048, medium=8192, high=16384
- When using budget-based thinking, increase `max_tokens` by the thinking budget
  (clamped to model max). Ensure at least 1024 output tokens remain after budget.

**Temperature constraint:** Do not send `temperature` when thinking is enabled.

Thinking blocks are preserved as `ContentBlock::RawOutput` and echoed back in subsequent
requests (same pattern as Codex reasoning items).

### Images

Base64-encoded in Anthropic format:
```json
{
  "type": "image",
  "source": {
    "type": "base64",
    "media_type": "image/png",
    "data": "..."
  }
}
```

### Prompt Caching

Mirror pi-mono's approach. Attach `cache_control: {type: "ephemeral"}` (5-min TTL) to:

1. **Each system prompt block** — stable across turns, always worth caching
2. **Last content block of the last user message** — standard Anthropic pattern that
   caches the entire conversation prefix for the next turn. Handles three sub-cases:
   - Array content: add to last block (text, image, or tool_result)
   - String content: convert to `[{type: "text", text: ..., cache_control: ...}]`
   - Only when last message role is `"user"`

No beta header needed (prompt caching is GA). No `cache_control` on tool definitions.

## Message Conversion

### Text Sanitization

Per pi-mono: pass all text content (user messages, assistant messages, system prompt)
through a surrogate sanitizer that removes unpaired Unicode surrogates (high surrogates
without matching low, or vice versa). These cause JSON serialization errors.

### Empty Content Handling

- Drop empty or whitespace-only text blocks and user messages
- Image-only user messages: prepend a placeholder `"(see attached image)"` text block

### Consecutive Tool Results

Per pi-mono: batch multiple consecutive `ToolResult` messages into a single `user`
message with multiple `tool_result` content blocks.

### Orphaned Tool Calls

If an assistant message has `tool_use` blocks but the following messages don't include
matching `tool_result` blocks, insert synthetic error results:
```json
{
  "type": "tool_result",
  "tool_use_id": "...",
  "content": "No result provided",
  "is_error": true
}
```

### Cross-Model Thinking Blocks

When conversation history contains thinking/redacted_thinking blocks from a different
model:
- **Redacted thinking blocks**: drop entirely (only valid for same model)
- **Thinking blocks with signature but no text**: drop
- **Non-empty thinking blocks**: convert to plain `text` blocks (strip signature)

### Error/Aborted Messages

Per pi-mono: skip assistant messages with `stop_reason` of `error` or `aborted` — remove
them entirely from the conversation before sending.

## Shared Code

After review, the Codex and Anthropic SSE parsers handle entirely different event types
and accumulation patterns. The only common code is trivial line splitting (~5 lines).
No shared `streaming.rs` — each provider keeps its own parser.

## Config

```toml
[models.aliases.claude]
provider = "anthropic"
model = "claude-sonnet-4-6-20250514"
```

Provider string `"anthropic"` added to `provider_for_alias()` match.

No default model. User must specify the model string.

## Error Handling

- 401 → `ProviderError::Auth` with hint about token expiry / re-login to Claude Code
- 429 → `ProviderError::RateLimited` with `retry-after` header
- 529 → `ProviderError::ServerError` (overloaded)
- Refresh failure → clear error telling user to open Claude Code to re-auth
- Circuit breaker: reuse existing `CircuitBreaker`
- On stream error: clean up block indexes, set stop_reason to `"error"` (or `"aborted"`
  if signal was aborted)

## Files

```
src/providers/anthropic/
├── mod.rs              — AnthropicProvider (Provider trait impl), provider construction
├── messages.rs         — message conversion (Ghost → Anthropic request format)
├── streaming.rs        — SSE stream parsing, response accumulation
├── credentials.rs      — credential reading, refresh, file locking
└── tool_names.rs       — Claude Code canonical tool name translation
```

- `src/providers/types.rs` — add `"anthropic"` to `provider_for_alias()`

## ToS Risk

This approach uses Claude Code OAuth tokens against the Anthropic API with header
spoofing. Anthropic could block this at any time. The Claude Code CLI subprocess
approach (`claude --print -`) can be added later as a ToS-compliant fallback.
