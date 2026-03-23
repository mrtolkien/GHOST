# Compaction Overhaul: Dynamic Turn Boundary + Context Overflow Recovery

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make compaction use the current turn as its boundary (not a fixed 20-message window), mask tool call inputs, and gracefully recover from context overflow errors instead of crashing.

**Architecture:** Four changes to the existing compaction system: (1) replace fixed `keep_window` with dynamic "current turn" detection, (2) extend masking to cover tool call inputs, (3) wire all chat handlers to use full compaction, (4) add `ContextOverflow` error detection + retry. The two-phase approach (mask → summarize) stays — only the split point and masking scope change.

**Design spec:** `backlog/tasks/2026-03-23-compaction-overhaul-design.md`

**Tech Stack:** Rust, existing compaction infrastructure in `src/chat/compaction.rs`, provider error types in `src/providers/types.rs`

---

## File Map

| File | Action | Responsibility |
|------|--------|---------------|
| `src/providers/types.rs` | Modify | Add `ContextOverflow` variant + `is_context_overflow_message()` |
| `src/providers/codex_responses.rs` | Modify | Detect context overflow in SSE + JSON error paths |
| `src/providers/openai_oauth.rs` | Modify | Detect context overflow in HTTP 400 path |
| `src/providers/openai_compatible_provider.rs` | Modify | Detect context overflow in HTTP 400 path |
| `src/providers/anthropic/mod.rs` | Modify | Detect context overflow in HTTP 400 path |
| `src/providers/anthropic/streaming.rs` | Modify | Detect context overflow in SSE error events |
| `src/chat/compaction.rs` | Modify | Replace `keep_window` with `find_current_turn_start()`, rename masking fn, add tool input masking, cap summarization input, remove `apply_masking_if_needed`, return bool from `compact_in_tool_loop_with_config` |
| `src/chat/session.rs` | Modify | `ChatHandler.post_tool_iteration` → full compaction, remove `keep_window` from `coding_compaction_config`, replace `apply_masking_if_needed` in agent pre-run (~line 1325), emit `Compacted` event |
| `src/chat/tool_loop.rs` | Modify | Catch `ContextOverflow`, rebuild request after compaction, retry once |
| `src/chat/types.rs` | Modify | Add `ToolLoopEvent::Compacted` variant |
| `src/interfaces/discord/ui_events.rs` | Modify | Render compaction event as Discord message |
| `src/config.rs` | Modify | Remove `keep_window` from `CompactionConfig`, `CompactionSettings`, `load()` default, and `test_config()` |

---

### Task 1: Add `ContextOverflow` error variant and detection

Detect context overflow from all providers using string matching on error messages inside each provider at parse time. Produces a typed `ProviderError::ContextOverflow` variant.

**Files:**
- Modify: `src/providers/types.rs:193-224`
- Modify: `src/providers/codex_responses.rs:271-284,336-345`
- Modify: `src/providers/openai_oauth.rs:229-233`
- Modify: `src/providers/openai_compatible_provider.rs:181-184`
- Modify: `src/providers/anthropic/mod.rs:230-234`
- Modify: `src/providers/anthropic/streaming.rs:160-163`
- Test: `src/providers/types.rs` (inline test)

- [ ] **Step 1: Write test for context overflow detection**

Add an inline test module at the bottom of `src/providers/types.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_context_overflow_detects_known_patterns() {
        // OpenAI Codex Responses API
        assert!(ProviderError::is_context_overflow_message(
            "Your input exceeds the context window of this model"
        ));
        // OpenAI Chat Completions API
        assert!(ProviderError::is_context_overflow_message(
            "This model's maximum context length is 128000 tokens"
        ));
        // Anthropic
        assert!(ProviderError::is_context_overflow_message(
            "prompt_length exceeded maximum of 200000"
        ));
        // Generic patterns
        assert!(ProviderError::is_context_overflow_message("too many tokens"));
        assert!(ProviderError::is_context_overflow_message("prompt is too long"));
        assert!(ProviderError::is_context_overflow_message(
            "input tokens exceed the configured limit"
        ));
        // Negative cases
        assert!(!ProviderError::is_context_overflow_message("rate limited"));
        assert!(!ProviderError::is_context_overflow_message("invalid api key"));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib providers::types::tests::is_context_overflow_detects_known_patterns`
Expected: FAIL — `is_context_overflow_message` does not exist.

- [ ] **Step 3: Add `ContextOverflow` variant and detection function**

In `src/providers/types.rs`, add the variant after `ServerError`:

```rust
    #[error("context overflow: {0}")]
    ContextOverflow(String),
```

Add the detection function as an associated function:

```rust
impl ProviderError {
    /// Check if an error message indicates a context window overflow.
    /// Provider-agnostic: matches known patterns from OpenAI, Anthropic, and others.
    pub fn is_context_overflow_message(msg: &str) -> bool {
        let lower = msg.to_lowercase();
        lower.contains("exceeds the context window")
            || lower.contains("context window of this model")
            || lower.contains("maximum context length")
            || lower.contains("context_length_exceeded")
            || lower.contains("context length exceeded")
            || lower.contains("too many tokens")
            || lower.contains("token limit exceeded")
            || lower.contains("prompt is too long")
            || lower.contains("input is too long")
            || lower.contains("prompt_length exceeded")
            || lower.contains("input tokens exceed")
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib providers::types::tests`
Expected: PASS

- [ ] **Step 5: Wire detection into all provider error paths**

In each provider, at the point where `ProviderError::InvalidResponse` is returned for
HTTP 400 or failed-response errors, check the message first:

**`src/providers/codex_responses.rs`** — two places (line ~281 JSON path, line ~342 SSE path):
```rust
let err_msg = format!("codex response failed: {error_msg}");
if ProviderError::is_context_overflow_message(error_msg) {
    return Err(ProviderError::ContextOverflow(err_msg));
}
return Err(ProviderError::InvalidResponse(err_msg));
```

**`src/providers/openai_oauth.rs`** line ~231, **`src/providers/openai_compatible_provider.rs`** line ~182, **`src/providers/anthropic/mod.rs`** line ~232:
```rust
let err_msg = format!("HTTP {status}: {response_body}");
if ProviderError::is_context_overflow_message(&response_body) {
    return Err(ProviderError::ContextOverflow(err_msg));
}
return Err(ProviderError::InvalidResponse(err_msg));
```

**`src/providers/anthropic/streaming.rs`** line ~162 (SSE error event):
```rust
if ProviderError::is_context_overflow_message(&msg) {
    return Err(ProviderError::ContextOverflow(msg.to_string()));
}
return Err(ProviderError::InvalidResponse(msg.to_string()));
```

- [ ] **Step 6: Run full test suite**

Run: `cargo test`
Expected: All existing tests pass.

- [ ] **Step 7: Commit**

```
git add src/providers/
git commit -m "feat: add ContextOverflow error variant with provider-agnostic detection"
```

---

### Task 2: Replace fixed `keep_window` with dynamic current-turn boundary

Replace the split point, extend masking to tool inputs, cap summarization input size,
remove `apply_masking_if_needed`, remove `keep_window` from config. Have
`compact_in_tool_loop_with_config` return a `bool` indicating whether Phase 2 ran (needed
for Discord notification in Task 3).

**Files:**
- Modify: `src/chat/compaction.rs`
- Modify: `src/config.rs:142-145,271-278,457-463,798-802`
- Modify: `src/chat/session.rs:784-798` (`coding_compaction_config`), `~1325` (agent pre-run)
- Test: `src/chat/compaction.rs` (inline tests)

- [ ] **Step 1: Write test for `find_current_turn_start`**

Add to the existing test module in `src/chat/compaction.rs`:

```rust
#[test]
fn find_current_turn_start_after_last_user_text() {
    let messages = vec![
        user_text("Hello"),                                    // 0
        assistant_text("Hi"),                                  // 1
        user_text("Search for X"),                             // 2
        assistant_with_tool("Searching", "tu_1", "web_search"),// 3
        tool_result("tu_1", "results..."),                     // 4
        assistant_with_tool("Fetching", "tu_2", "web_fetch"),  // 5
        tool_result("tu_2", "page content..."),                // 6
    ];
    assert_eq!(find_current_turn_start(&messages), 2);
}

#[test]
fn find_current_turn_start_no_user_message() {
    let messages = vec![assistant_text("Hi")];
    assert_eq!(find_current_turn_start(&messages), 0);
}

#[test]
fn find_current_turn_start_tool_result_only_user_messages() {
    let messages = vec![
        user_text("Do something"),                              // 0
        assistant_with_tool("OK", "tu_1", "shell"),             // 1
        tool_result("tu_1", "output"),                          // 2
        assistant_with_tool("More", "tu_2", "shell"),           // 3
        tool_result("tu_2", "output2"),                         // 4
    ];
    assert_eq!(find_current_turn_start(&messages), 0);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib chat::compaction::tests::find_current_turn_start`
Expected: FAIL — function does not exist.

- [ ] **Step 3: Implement `find_current_turn_start`**

```rust
/// Find the index of the last user message that contains actual text
/// (not just tool results). Everything from this index onward is the
/// "current turn" and should be preserved verbatim during compaction.
#[must_use]
pub fn find_current_turn_start(messages: &[ChatMessage]) -> usize {
    for (i, msg) in messages.iter().enumerate().rev() {
        if msg.role != Role::User {
            continue;
        }
        let has_text = msg.content.iter().any(|block| {
            matches!(block, ContentBlock::Text { text } if !text.is_empty())
        });
        if has_text {
            return i;
        }
    }
    0
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib chat::compaction::tests::find_current_turn_start`
Expected: PASS

- [ ] **Step 5: Write test for tool input masking**

```rust
#[test]
fn mask_includes_tool_use_inputs() {
    let messages = vec![
        user_text("Hello"),
        assistant_with_tool("Let me search", "tu_1", "web_search"),
        tool_result("tu_1", &"x".repeat(500)),
        user_text("Thanks"),
    ];

    // keep_start=3 → only last message kept, first 3 masked
    let masked = mask_tool_interactions(&messages, 3, 100);

    // Tool result should be masked
    if let ContentBlock::ToolResult { content, .. } = &masked[2].content[0] {
        assert!(content.contains("[tool_result:"));
    } else {
        panic!("expected tool result");
    }

    // Tool use input should be replaced with {}
    if let ContentBlock::ToolUse { input, .. } = &masked[1].content[1] {
        assert_eq!(input.to_string(), "{}");
    } else {
        panic!("expected tool use at index 1 content 1");
    }
}
```

- [ ] **Step 6: Run test to verify it fails**

Run: `cargo test --lib chat::compaction::tests::mask_includes_tool_use_inputs`
Expected: FAIL — `mask_tool_interactions` does not exist.

- [ ] **Step 7: Rename `mask_tool_results` → `mask_tool_interactions`, add `ToolUse` masking**

Rename the function and add a `ToolUse` arm that replaces `input` with `json!({})`:

```rust
ContentBlock::ToolUse { id, name, .. } => ContentBlock::ToolUse {
    id: id.clone(),
    name: name.clone(),
    input: serde_json::json!({}),
},
```

The rest of the function body stays the same (ToolResult masking, Image masking,
`other => other.clone()` passthrough).

Update the 2 remaining call sites that use `mask_tool_results`:
- `compact_if_needed` (~line 471)
- `compact_in_tool_loop_with_config` (~line 603)

Delete `apply_masking_if_needed` entirely (~lines 544-559) — it's replaced by
`compact_in_tool_loop` everywhere:
- `ChatHandler::post_tool_iteration` at session.rs:758 (rewired in Task 3)
- Agent pre-run at session.rs:1325 — replace with:
  ```rust
  self.compact_in_tool_loop(&session_thing, &mut history).await;
  ```

- [ ] **Step 8: Replace `keep_window` with `find_current_turn_start` in all 3 locations**

**Location 1** — `compact_if_needed` (~line 470):
```rust
// Before:
let keep_start = history.len().saturating_sub(compaction.keep_window);
// After:
let keep_start = find_current_turn_start(history);
```

**Location 2** — `compact_in_tool_loop_with_config` (~line 602):
```rust
// Before:
let keep_start = history.len().saturating_sub(compaction.keep_window);
// After:
let keep_start = find_current_turn_start(history);
```

**Location 3** — `summarize_older_messages` (~line 342):
```rust
// Before:
let split = messages.len().saturating_sub(config.keep_window);
// After:
let split = find_current_turn_start(messages);
```

Also fix the `#[tracing::instrument]` on `summarize_older_messages` (~line 329):
```rust
// Before:
fields(total_messages = messages.len(), keep_window = config.keep_window)
// After:
fields(total_messages = messages.len())
```

- [ ] **Step 9: Cap summarization input size**

In `summarize_older_messages`, after `render_messages_for_summary`, truncate from the
beginning if too large:

```rust
const MAX_SUMMARIZATION_INPUT_CHARS: usize = 50_000;

let conversation_text = render_messages_for_summary(to_summarize, config.mask_preview_chars);
let conversation_text = if conversation_text.len() > MAX_SUMMARIZATION_INPUT_CHARS {
    let start = conversation_text.len() - MAX_SUMMARIZATION_INPUT_CHARS;
    format!("[earlier conversation truncated]\n\n{}", &conversation_text[start..])
} else {
    conversation_text
};
```

- [ ] **Step 10: Make `compact_in_tool_loop_with_config` return `bool`**

Change the return type from `()` to `bool`. Return `true` when Phase 2 summarization
ran successfully, `false` otherwise (including when compaction wasn't needed, or Phase 1
was sufficient, or Phase 2 failed).

Update `compact_in_tool_loop` wrapper to also return `bool` (pass through the inner
return value).

Update callers that currently ignore the return value:
- `CodingHandler::post_tool_iteration` (~session.rs:899) — capture but ignore (or emit
  Compacted event, same as ChatHandler in Task 3)
- `LuaAgentHandler::post_tool_iteration` (~session.rs:1028) — capture but ignore
- Agent pre-run (~session.rs:1325) — capture but ignore

- [ ] **Step 11: Remove `keep_window` from config**

In `src/config.rs`, remove `keep_window` from:
1. `CompactionSettings` struct (line 144): remove `pub keep_window: Option<usize>`
2. `CompactionConfig` struct (line 273): remove `pub keep_window: usize`
3. `load()` default construction (~line 459-463): remove the `keep_window:` field
4. `test_config()` (~line 800): remove `keep_window: 20` from the `CompactionConfig`

In `src/chat/session.rs`, update `coding_compaction_config()` (~line 784-798):
remove `keep_window: 12`, keep the `instructions` override.

- [ ] **Step 12: Run full test suite and fix compilation issues**

Run: `cargo test`

Expected breakages and fixes:
- Tests that call `mask_tool_results` → rename to `mask_tool_interactions`
- Tests that construct `CompactionConfig` with `keep_window` → remove the field
- `test_config()` in `config.rs` — remove `keep_window` from the constructed config
- Any test using `compaction.keep_window` directly → remove

- [ ] **Step 13: Commit**

```
git add src/chat/compaction.rs src/config.rs src/chat/session.rs
git commit -m "refactor: replace fixed keep_window with dynamic current-turn boundary

Compaction now masks/summarizes everything before the last user message
instead of using a fixed 20-message window. Also masks tool call inputs
(not just results), caps summarization input at 50K chars, and removes
the apply_masking_if_needed method."
```

---

### Task 3: Wire `ChatHandler` to full compaction + emit Discord notification

Fix the `ChatHandler` bug and add the compaction event for Discord.

**Files:**
- Modify: `src/chat/session.rs:753-776`
- Modify: `src/chat/types.rs:85-89`
- Modify: `src/interfaces/discord/ui_events.rs:46-58`

- [ ] **Step 1: Add `Compacted` variant to `ToolLoopEvent`**

In `src/chat/types.rs`:

```rust
pub enum ToolLoopEvent {
    ToolCalls { calls: Vec<ToolCallInfo> },
    ToolResults { results: Vec<ToolResultInfo> },
    TodoUpdated { items: Vec<TodoItem> },
    Compacted,
}
```

- [ ] **Step 2: Handle the event in Discord UI renderer**

In `src/interfaces/discord/ui_events.rs`, add a match arm in `run()` (~line 46-58):

```rust
ToolLoopEvent::Compacted => {
    self.handle_compacted().await;
}
```

Add the handler method (follows the `handle_tool_calls` pattern — uses
`send_v2_message` with `container` + `text_display`):

```rust
/// Muted grey for compaction notices.
const COMPACTION_COLOR: u32 = 0x58_5B_70;

async fn handle_compacted(&self) {
    let components = vec![container(
        vec![text_display(
            "context compacted — older conversation was summarized to fit the model's context window",
        )],
        Some(COMPACTION_COLOR),
    )];
    if let Err(e) = send_v2_message(&self.http, self.channel_id, &components, Vec::new()).await {
        tracing::warn!(error = e.to_string(), "failed to send compaction message");
    }
}
```

- [ ] **Step 3: Replace `apply_masking_if_needed` with `compact_in_tool_loop` in `ChatHandler`**

In `ChatHandler::post_tool_iteration` (session.rs ~line 753-776), replace:
```rust
self.session_chat.apply_masking_if_needed(history);
```
with:
```rust
let compacted = self.session_chat
    .compact_in_tool_loop(self.session_thing, history)
    .await;
if compacted {
    if let Some(tx) = self.event_tx {
        let _ = tx.send(ToolLoopEvent::Compacted);
    }
}
```

Do the same in `CodingHandler::post_tool_iteration` (~line 893-900) and
`LuaAgentHandler::post_tool_iteration` (~line 1021-1029) — they already call
`compact_in_tool_loop`/`compact_in_tool_loop_with_config`, just capture the new `bool`
return and emit the event.

- [ ] **Step 4: Run tests**

Run: `cargo test`
Expected: PASS

- [ ] **Step 5: Commit**

```
git add src/chat/session.rs src/chat/types.rs src/interfaces/discord/ui_events.rs
git commit -m "fix: ChatHandler uses full compaction + Discord notification on compact

ChatHandler now runs Phase 1+2 compaction during tool loops (was Phase 1
only). Emits a system message to Discord when summarization runs so the
user knows context was compacted."
```

---

### Task 4: Catch `ContextOverflow` in the tool loop and retry

When the provider returns `ContextOverflow`, force full compaction, rebuild the request
with the compacted history, and retry once.

**Files:**
- Modify: `src/chat/tool_loop.rs:194-249`

- [ ] **Step 1: Add `ContextOverflow` arm in the response match**

In `run_tool_loop`, inside the `match` on the provider response (line 194-249), add a
new arm after the `ServerError` retry arm (line 201-222), before the catch-all
`Ok(Err(e))` at line 223.

**IMPORTANT:** After compaction, `history` has changed but `request` still contains the
old `messages`. Must rebuild the request with the compacted history:

```rust
Ok(Err(ProviderError::ContextOverflow(msg))) => {
    tracing::warn!(
        error = msg.clone(),
        iteration = iterations as u64,
        "context overflow — forcing compaction and retrying",
    );
    handler.post_tool_iteration(history, 0).await?;

    // Rebuild request with compacted history
    let retry_request = ChatRequest {
        messages: history.clone(),
        ..request
    };
    match tokio::time::timeout(
        PROVIDER_REQUEST_TIMEOUT,
        session_chat.provider().chat(retry_request),
    )
    .await
    {
        Ok(result) => result.map_err(ChatError::from)?,
        Err(_elapsed) => {
            return Err(ChatError::Provider(ProviderError::Timeout {
                seconds: PROVIDER_REQUEST_TIMEOUT.as_secs(),
            }));
        }
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test`
Expected: PASS

- [ ] **Step 3: Commit**

```
git add src/chat/tool_loop.rs
git commit -m "feat: catch context overflow errors and retry after compaction

When a provider returns context_length_exceeded, forces full compaction
(masking + summarization) on the history, rebuilds the request with the
compacted messages, and retries once. If it still fails, propagates the
error normally."
```
