# Spec 25: Provider Reliability Improvements

## Summary

Several small improvements to provider reliability and observability, grouped together
because they're all changes to the provider layer.

## 1. Remove Empty Response Retries from Providers

### Problem

All three provider implementations (OpenRouter, Kimi, OpenAI OAuth) have identical
retry-on-empty-response logic: up to 2 retries with exponential backoff when the
provider returns `EmptyResponse`. This was added to work around flaky provider behavior
but is wrong for two reasons:

1. **Hides real bugs.** If our request is malformed, retrying won't help — it just
   delays the error by 6 seconds (2s + 4s backoff).
2. **Duplicated code.** The same retry loop is copy-pasted across three providers.
3. **The tool loop already handles recovery.**

### Changes

- Remove `empty_response_retries` field from `OpenAiCompatibleProvider`
- Remove retry loop from `Provider::chat()` in all three providers — just call
  `send_request` directly
- Remove `DEFAULT_EMPTY_RESPONSE_RETRIES` constants
- Update `new_for_tests` signatures (drop the retries parameter)

## 2. Empty EndTurn Recovery in Tool Loop

### Problem

Some models (especially via OpenRouter) occasionally return an empty EndTurn — no text,
no tool calls. The tool loop currently returns this as an empty `ChatResult`, which is
useless.

### Changes

In `src/chat/tool_loop.rs`, when `StopReason::EndTurn` is received with empty text and
no tool calls, and iterations remain:

1. Log a warning
2. Push the empty assistant response to history
3. Push a user message: "Your previous response was empty. Continue your work and use
   your tools."
4. Continue the loop

This only fires when there's genuinely nothing in the response. If the model sends text
(even short), it's accepted normally.

## 3. Codex Response Parser Improvements

### Problem

The codex responses parser (`src/providers/codex_responses.rs`) discards some items
silently:

- Empty `message` items (content: []) are dropped, losing context
- `function_call` items with missing fields are dropped silently
- The "reasoning-only response" case returns `EmptyResponse`, but reasoning blocks
  should be preserved as `RawOutput` so the tool loop can handle recovery

### Changes

- Empty message items → preserved as `RawOutput` with type "message"
- Malformed function_call items → preserved as `RawOutput` with warning log
- Reasoning-only responses → no longer `EmptyResponse`; return content with `RawOutput`
  blocks, let the tool loop's empty-EndTurn recovery handle it

## 4. Cache Token Logging

### Problem

Provider response logging doesn't include cache read/creation tokens, making it hard to
verify that prompt caching is working.

### Changes

Add `cache_read_tokens` and `cache_creation_tokens` fields to the `logfire::info!` calls
in `OpenAiCompatibleProvider::send_request` and `OpenAiOAuthProvider::send_request`.

## 5. Brave Search Retry Simplification

### Problem

`BraveSearchProvider::search` uses a two-method pattern (`search` + `execute_request`)
with a custom `SearchRequestError` enum to handle rate limiting. The abstraction is
unnecessary — the retry logic can be a simple loop.

### Changes

- Merge `execute_request` back into `search`
- Replace `SearchRequestError` enum with a simple retry loop (max 2 retries, exponential
  backoff from 2s)
- Rate-limited responses (429) trigger retry; all other errors return immediately

## Files

| File                                          | Change                                    |
| --------------------------------------------- | ----------------------------------------- |
| `src/providers/openai_compatible_provider.rs` | Remove retry loop, add cache token log    |
| `src/providers/openrouter.rs`                 | Remove retry constant, update constructor |
| `src/providers/kimi_code.rs`                  | Remove retry constant, update constructor |
| `src/providers/openai_oauth.rs`               | Remove retry loop, add cache token log    |
| `src/providers/codex_responses.rs`            | Preserve empty items as RawOutput         |
| `src/chat/tool_loop.rs`                       | Empty EndTurn recovery                    |
| `src/web/search.rs`                           | Simplify Brave search retries             |
| `tests/chat_orchestration_live.rs`            | May need update for retry changes         |
| `tests/providers/*.rs`                        | Update constructor calls                  |
