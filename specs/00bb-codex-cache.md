# Codex Response Chaining (`previous_response_id`)

## Context

The Codex Responses API supports response chaining: when `previous_response_id` is set
on a request, the server already has the prior conversation context cached. The `input`
only needs to contain new messages since that response, dramatically reducing request
payload size and improving cache hit rates during tool loops (where 5+ iterations send
near-identical history).

Currently every tool loop iteration sends the full message history. This change enables
chaining for the Codex (openai_oauth) provider only.

## Changes

### 1. `src/providers/types.rs` — ChatRequest + Provider trait

- Add `previous_response_id: Option<String>` to `ChatRequest` (with `#[serde(skip)]`)
- Add `fn supports_response_chaining(&self) -> bool` to `Provider` trait, default
  `false`

### 2. `src/providers/codex_responses.rs` — Request building

- Add
  `#[serde(skip_serializing_if = "Option::is_none")] previous_response_id: Option<String>`
  to `CodexResponsesRequest`
- Change `store: false` to `store: true` (required for server to retain responses)
- In `build_codex_request_body()`: pass through `previous_response_id` from
  `ChatRequest`
- No input slicing here — the tool loop handles that by passing only new messages

### 3. `src/providers/openai_oauth.rs` — Enable chaining

- Override `supports_response_chaining() -> true` in the `Provider` impl

### 4. `src/chat/tool_loop.rs` — Chaining orchestration

Add two tracking variables:

```rust
let mut last_response_id: Option<String> = None;
let mut history_len_at_last_request: usize = 0;
let chaining_enabled = session_chat.provider().supports_response_chaining();
```

When building `ChatRequest`:

- If `chaining_enabled && last_response_id.is_some()`: set `previous_response_id`, use
  `history[history_len_at_last_request..].to_vec()` as messages
- Otherwise: full `history.clone()` as today

After each successful provider response:

- Record `history_len_at_last_request = history.len()` (before appending new messages)
- Capture `last_response_id = response.response_id.clone()`

Error handling:

- On any provider error, clear `last_response_id` (chain is broken). The retry logic
  already retries with `request.clone()` — since we set `previous_response_id` on the
  request, we need to ensure the retry uses full history. Handle this by: if the first
  attempt with chaining fails, clear chaining state and rebuild request with full
  history for the retry.

### 5. Test updates

- All `ChatRequest { .. }` constructors in tests get `previous_response_id: None` (or
  use `..Default::default()`)
- Add unit test in `codex_responses.rs`: verify `previous_response_id` and `store: true`
  appear in serialized request when set
- Add unit test: verify input contains only new messages when chaining

## Files to modify

| File                               | Changes                                                      |
| ---------------------------------- | ------------------------------------------------------------ |
| `src/providers/types.rs`           | Add field to ChatRequest, trait method                       |
| `src/providers/codex_responses.rs` | Add field to request struct, store: true, passthrough        |
| `src/providers/openai_oauth.rs`    | Override supports_response_chaining                          |
| `src/chat/tool_loop.rs`            | Chaining tracking, conditional message slicing               |
| `src/chat/compaction.rs`           | Add `previous_response_id: None` to ChatRequest constructor  |
| `tests/providers/*.rs`             | Add `previous_response_id: None` to ChatRequest constructors |

## Verification

1. `just ci` — all existing tests pass
2. New unit tests for chaining serialization
3. Live tests
4. Manual: send multi-turn messages via Discord, check Logfire for
   `previous_response_id` being set on iterations > 0
