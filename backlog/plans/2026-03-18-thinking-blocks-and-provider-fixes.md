# Thinking Blocks & Provider Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix three production bugs — thinking block ordering crash, circuit breaker triggering on client errors, and compaction summary "system" role rejection — by introducing a typed `ContentBlock::Thinking` variant, hardening the circuit breaker, and fixing Anthropic-specific system message handling.

**Architecture:** Replace the opaque `ContentBlock::RawOutput` for thinking/reasoning blocks with a typed `ContentBlock::Thinking { text, signature, opaque_data }` variant. Each provider reconstructs its native format from these fields, and cross-model transitions degrade to readable text. Circuit breaker only fires on transient provider errors (429, 5xx, network). Compaction summaries are converted from `Role::System` to a user-role text block in the Anthropic provider's message conversion, keeping session code provider-agnostic.

**Tech Stack:** Rust, serde_json, Anthropic Messages API, OpenAI Codex Responses API

**Relevant test infrastructure:** `@testing` skill, live tests with `--features live-tests-llms`

---

## File Map

| File | Action | Responsibility |
|------|--------|---------------|
| `src/providers/types.rs` | Modify | Add `Thinking` variant to `ContentBlock` |
| `src/providers/anthropic/streaming.rs` | Modify | Produce `Thinking` instead of `RawOutput` |
| `src/providers/anthropic/messages.rs` | Modify | Consume `Thinking` (reconstruct + order first); handle `Role::System` as user message |
| `src/providers/codex_responses.rs` | Modify | Produce `Thinking` for reasoning items; consume `Thinking` in request builder |
| `src/providers/openai_compatible.rs` | Modify | Consume `Thinking` (cross-model text fallback) |
| `src/chat/convert.rs` | Modify | DB round-trip: store/load `Thinking` via `raw_output` column; update `raw_output_to_values` |
| `src/chat/compaction.rs` | Modify | Token estimation and summary rendering for `Thinking` |
| `src/chat/transcript.rs` | Modify | Render `Thinking` blocks in transcript |
| `src/providers/anthropic/mod.rs` | Modify | Remove `record_failure` from 400 catch-all |
| `src/providers/openai_oauth.rs` | Modify | Remove `record_failure` from 400 catch-all |
| `src/providers/openai_compatible_provider.rs` | Modify | Remove `record_failure` from 400 catch-all |
| `tests/providers/anthropic_live.rs` | Modify | Update round-trip test to use `Thinking` variant |
| `tests/providers/reasoning_live.rs` | Modify | Update to expect `Thinking` variant |

---

## Task 1: Add `ContentBlock::Thinking` variant

**Files:**
- Modify: `src/providers/types.rs:102-133`

- [ ] **Step 1: Add the `Thinking` variant to the `ContentBlock` enum**

In `src/providers/types.rs`, add after the `Image` variant (before `RawOutput`):

```rust
/// Model reasoning/thinking block. Typed for correct ordering and
/// cross-model transitions.
///
/// - Anthropic `thinking`: text + signature (readable, verifiable)
/// - Anthropic `redacted_thinking`: opaque_data only
/// - OpenAI `reasoning`: opaque_data (encrypted) + text (summary)
Thinking {
    /// Human-readable reasoning text.
    text: Option<String>,
    /// Anthropic thinking signature for round-trip verification.
    signature: Option<String>,
    /// Opaque/encrypted data (Anthropic redacted_thinking `data`,
    /// OpenAI reasoning `encrypted_content`).
    opaque_data: Option<String>,
},
```

Keep `RawOutput` — it still handles truly unknown provider outputs (e.g. `function_call` fallbacks in Codex).

- [ ] **Step 2: Fix all exhaustive match arms**

Run `cargo check 2>&1 | head -60` to find every non-exhaustive `match` on `ContentBlock`. Add `ContentBlock::Thinking { .. }` arms alongside the existing `RawOutput` handling — for now just mirror what `RawOutput` did. Files expected to need fixes:

- `src/chat/compaction.rs` (token estimation + summary rendering)
- `src/providers/anthropic/messages.rs` (convert_content_blocks)
- `src/providers/openai_compatible.rs` (convert_messages)
- `src/providers/codex_responses.rs` (request builder loop)

For each, add a placeholder arm that delegates to the same logic as RawOutput. These will be refined in later tasks.

- [ ] **Step 3: Verify compilation**

Run: `cargo check`
Expected: compiles with no errors (warnings OK)

- [ ] **Step 4: Commit**

```
feat: add ContentBlock::Thinking variant for typed reasoning blocks
```

---

## Task 2: Produce `Thinking` blocks from Anthropic streaming parser

**Files:**
- Modify: `src/providers/anthropic/streaming.rs:246-257`
- Test: existing tests in same file + `tests/providers/anthropic_live.rs`

- [ ] **Step 1: Update the streaming parser to emit `Thinking` instead of `RawOutput`**

In `src/providers/anthropic/streaming.rs`, replace the `"thinking"` and `"redacted_thinking"` arms in `build_content_block()` (around line 246):

```rust
"thinking" => ContentBlock::Thinking {
    text: Some(state.thinking.clone()),
    signature: Some(state.signature.clone()),
    opaque_data: None,
},
"redacted_thinking" => ContentBlock::Thinking {
    text: None,
    signature: None,
    opaque_data: state.redacted_json
        .as_ref()
        .and_then(|v| v.get("data"))
        .and_then(Value::as_str)
        .map(String::from),
},
```

- [ ] **Step 2: Fix the unit tests in the same file**

Update the tests that match on `ContentBlock::RawOutput` with `original_type == "thinking"` or `"redacted_thinking"` to instead match on `ContentBlock::Thinking { text, signature, opaque_data }` and assert the correct fields.

- [ ] **Step 3: Run unit tests**

Run: `cargo test --lib providers::anthropic::streaming`
Expected: all pass

- [ ] **Step 4: Run the live round-trip test**

Run: `cargo test --features live-tests-llms anthropic_thinking_block_round_trip -- --nocapture`
Expected: FAIL (test still constructs/matches `RawOutput` — will fix in Task 7)

- [ ] **Step 5: Commit**

```
feat: Anthropic streaming parser produces ContentBlock::Thinking
```

---

## Task 3: Produce `Thinking` blocks from Codex Responses parser

**Files:**
- Modify: `src/providers/codex_responses.rs:477-487`
- Test: `tests/providers/reasoning_live.rs`

- [ ] **Step 1: Update the Codex parser to emit `Thinking` for reasoning items**

In `src/providers/codex_responses.rs`, in the `other =>` match arm (line 477), check if `other == "reasoning"` and produce a `Thinking` block:

```rust
other => {
    if other == "reasoning" {
        let text = {
            let summary = extract_reasoning_summary(item);
            if summary.is_empty() { None } else { Some(summary) }
        };
        let opaque_data = item
            .get("encrypted_content")
            .and_then(Value::as_str)
            .map(String::from);
        logfire::info!(
            "codex: preserving reasoning block",
            has_text = text.is_some(),
            has_opaque = opaque_data.is_some(),
        );
        content.push(ContentBlock::Thinking {
            text,
            signature: None,
            opaque_data,
        });
    } else {
        let reasoning_summary = extract_reasoning_summary(item);
        logfire::info!(
            "codex: preserving opaque output item",
            item_type = other.to_string(),
            reasoning_summary = reasoning_summary,
        );
        content.push(ContentBlock::RawOutput {
            original_type: other.to_string(),
            value: item.clone(),
        });
    }
}
```

- [ ] **Step 2: Fix the reasoning live test**

Update `tests/providers/reasoning_live.rs` to match on `ContentBlock::Thinking` instead of `ContentBlock::RawOutput` with `original_type == "reasoning"`.

- [ ] **Step 3: Run tests**

Run: `cargo test --lib providers::codex && cargo test --features live-tests-llms reasoning`
Expected: unit tests pass; live test pass if credentials available

- [ ] **Step 4: Commit**

```
feat: Codex Responses parser produces ContentBlock::Thinking for reasoning
```

---

## Task 4: Consume `Thinking` in Anthropic message builder (reconstruct + order)

This is the critical fix for the production bug — thinking blocks must come FIRST in assistant messages.

**Files:**
- Modify: `src/providers/anthropic/messages.rs:243-324` (`convert_content_blocks`)
- Modify: `src/providers/anthropic/messages.rs:332-338` (`role_str` — handle System role)

- [ ] **Step 1: Handle `Thinking` in `convert_content_blocks`**

In `convert_content_blocks`, replace the `RawOutput` arm's thinking/redacted handling and add a `Thinking` arm:

```rust
ContentBlock::Thinking {
    text,
    signature,
    opaque_data,
} => {
    if let (Some(text), Some(sig)) = (text, signature) {
        // Anthropic thinking: reconstruct with signature
        out.push(json!({
            "type": "thinking",
            "thinking": text,
            "signature": sig,
        }));
    } else if opaque_data.is_some() && text.is_none() && signature.is_none() {
        // Anthropic redacted_thinking: no readable text, has opaque blob.
        // Only reconstruct as redacted_thinking when there's NO text
        // and NO signature — this distinguishes it from Codex reasoning
        // (which has text + opaque_data but no signature).
        out.push(json!({
            "type": "redacted_thinking",
            "data": opaque_data.as_ref().unwrap(),
        }));
    } else if let Some(text) = text {
        // Cross-model block (e.g. Codex reasoning sent to Anthropic).
        // Convert readable text to a plain text note so context survives.
        out.push(json!({
            "type": "text",
            "text": format!("[Reasoning]: {text}"),
        }));
        has_text = true;
    }
    // If nothing matched (all None), skip silently.
}
```

Update the existing `RawOutput` arm to only handle non-thinking types (remove the `thinking`/`redacted_thinking` checks since those are now `Thinking` blocks).

- [ ] **Step 2: Ensure thinking blocks are ordered FIRST**

After the `for block in blocks` loop completes but before returning `out`, partition the output so thinking/redacted_thinking blocks come before all others:

```rust
// Thinking blocks must precede other content per Anthropic API.
let mut thinking = Vec::new();
let mut rest = Vec::new();
for block in out {
    let block_type = block.get("type").and_then(Value::as_str).unwrap_or("");
    if block_type == "thinking" || block_type == "redacted_thinking" {
        thinking.push(block);
    } else {
        rest.push(block);
    }
}
thinking.extend(rest);
let out = thinking;
```

Place this just before the image-only placeholder check.

- [ ] **Step 3: Handle `Role::System` compaction summaries**

In `convert_messages` (line 109), when a message has `Role::System`, convert it to a user message instead of passing `"system"` as the role:

```rust
let role_str = match msg.role {
    Role::System => "user",  // Anthropic doesn't accept system in messages
    _ => role_str(&msg.role),
};
```

And wrap the system message text to make it clear it's a summary:

```rust
let content_blocks = if msg.role == Role::System {
    // Convert system messages (e.g. compaction summaries) to user
    // text blocks — Anthropic requires system in top-level param.
    msg.content
        .iter()
        .filter_map(|b| match b {
            ContentBlock::Text { text } => {
                let sanitized = sanitize_surrogates(text);
                if sanitized.is_empty() {
                    None
                } else {
                    Some(json!({
                        "type": "text",
                        "text": format!("[Previous conversation summary]\n{sanitized}"),
                    }))
                }
            }
            _ => None,
        })
        .collect::<Vec<_>>()
} else {
    convert_content_blocks(&msg.content, ghost_tool_names)
};
```

- [ ] **Step 4: Write unit tests for thinking block ordering**

Add a test to `messages.rs` tests:

```rust
#[test]
fn thinking_blocks_ordered_before_tool_use() {
    let req = simple_request(vec![
        ChatMessage {
            role: Role::User,
            content: vec![ContentBlock::Text { text: "hi".into() }],
        },
        ChatMessage {
            role: Role::Assistant,
            content: vec![
                ContentBlock::ToolUse {
                    id: "t1".into(),
                    name: "bash".into(),
                    input: json!({}),
                },
                ContentBlock::Thinking {
                    text: Some("let me think".into()),
                    signature: Some("sig123".into()),
                    opaque_data: None,
                },
            ],
        },
        ChatMessage {
            role: Role::User,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "t1".into(),
                content: "ok".into(),
                is_error: false,
            }],
        },
    ]);
    let body = build_request_body(&req, &["bash"]).unwrap();
    let messages = body["messages"].as_array().unwrap();
    let assistant = &messages[1];
    let content = assistant["content"].as_array().unwrap();
    assert_eq!(content[0]["type"], "thinking", "thinking must come first");
    assert_eq!(content[1]["type"], "tool_use", "tool_use must come after thinking");
}

#[test]
fn system_role_converted_to_user_for_anthropic() {
    let req = simple_request(vec![
        ChatMessage {
            role: Role::System,
            content: vec![ContentBlock::Text {
                text: "Summary of prior conversation.".into(),
            }],
        },
        ChatMessage {
            role: Role::User,
            content: vec![ContentBlock::Text { text: "hello".into() }],
        },
    ]);
    let body = build_request_body(&req, &[]).unwrap();
    let messages = body["messages"].as_array().unwrap();
    assert_eq!(messages[0]["role"], "user");
    assert!(
        messages[0]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("conversation summary"),
    );
}
```

- [ ] **Step 5: Write cross-model unit test**

Test that Codex-originated `Thinking` blocks (text + opaque_data, no signature) are NOT
incorrectly reconstructed as `redacted_thinking`:

```rust
#[test]
fn cross_model_codex_reasoning_becomes_text_not_redacted() {
    let req = simple_request(vec![
        ChatMessage {
            role: Role::User,
            content: vec![ContentBlock::Text { text: "hi".into() }],
        },
        ChatMessage {
            role: Role::Assistant,
            content: vec![
                ContentBlock::Thinking {
                    text: Some("step by step reasoning".into()),
                    signature: None,
                    opaque_data: Some("encrypted_blob".into()),
                },
                ContentBlock::Text { text: "answer".into() },
            ],
        },
        ChatMessage {
            role: Role::User,
            content: vec![ContentBlock::Text { text: "ok".into() }],
        },
    ]);
    let body = build_request_body(&req, &[]).unwrap();
    let messages = body["messages"].as_array().unwrap();
    let assistant = &messages[1];
    let content = assistant["content"].as_array().unwrap();
    // Should be converted to text, not redacted_thinking
    assert!(content.iter().all(|b| b["type"] != "redacted_thinking"));
    assert!(content.iter().any(|b| {
        b["type"] == "text"
            && b["text"]
                .as_str()
                .unwrap_or("")
                .contains("step by step reasoning")
    }));
}
```

- [ ] **Step 6: Run all Anthropic unit tests**

Run: `cargo test --lib providers::anthropic`
Expected: all pass

- [ ] **Step 7: Commit**

```
fix: Anthropic thinking block ordering + system role handling
```

---

## Task 5: Consume `Thinking` in Codex request builder and OpenAI compatible provider

**Files:**
- Modify: `src/providers/codex_responses.rs:91-102` (request builder)
- Modify: `src/providers/openai_compatible.rs:226-241` (cross-model fallback)

- [ ] **Step 1: Update Codex request builder to reconstruct reasoning from `Thinking`**

In `build_codex_request_body`, replace the `RawOutput` collection loop (lines 93-98):

```rust
// Collect thinking blocks — reconstruct as reasoning input items.
let mut raw_items = Vec::new();
for block in &message.content {
    match block {
        ContentBlock::Thinking {
            text,
            opaque_data,
            ..
        } => {
            // Reconstruct Codex reasoning item
            let mut item = json!({"type": "reasoning"});
            if let Some(data) = opaque_data {
                item["encrypted_content"] = json!(data);
            }
            if let Some(text) = text {
                item["summary"] = json!([{"type": "summary_text", "text": text}]);
            }
            raw_items.push(item);
        }
        ContentBlock::RawOutput { value, .. } => {
            raw_items.push(value.clone());
        }
        _ => {}
    }
}
for raw in raw_items {
    input.push(CodexInputItem::Raw(raw));
}
```

- [ ] **Step 2: Update OpenAI compatible provider to use `Thinking` text**

In `src/providers/openai_compatible.rs`, update the content block loop (around line 226):

```rust
ContentBlock::Thinking { text, .. } => {
    if let Some(text) = text {
        text_parts.push(format!("[Reasoning]: {text}"));
    }
}
ContentBlock::RawOutput {
    original_type,
    value,
} => {
    let extracted = super::codex_responses::extract_reasoning_summary(value);
    if !extracted.is_empty() {
        text_parts.push(format!("[Previous model {original_type}]: {extracted}"));
    }
}
```

- [ ] **Step 3: Run unit tests**

Run: `cargo test --lib providers`
Expected: all pass

- [ ] **Step 4: Commit**

```
feat: Codex + OpenAI providers consume ContentBlock::Thinking
```

---

## Task 6: DB round-trip and chat layer updates

**Files:**
- Modify: `src/chat/convert.rs:42-108` (load), `src/chat/convert.rs:151-170` (`raw_output_to_values`)
- Modify: `src/chat/compaction.rs:36-48` (token estimation), `src/chat/compaction.rs:295-300` (summary rendering)
- Modify: `src/chat/transcript.rs:29-39` (transcript rendering)

- [ ] **Step 1: Update `raw_output_to_values` to handle `Thinking` blocks**

In `src/chat/convert.rs`, update `raw_output_to_values` to serialize `Thinking` blocks into the same JSON format used for DB storage:

```rust
pub(super) fn raw_output_to_values(content: &[ContentBlock]) -> Option<Vec<Value>> {
    let values: Vec<Value> = content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Thinking {
                text,
                signature,
                opaque_data,
            } => {
                // Determine original_type for DB storage
                let original_type = if signature.is_some() {
                    "thinking"
                } else if opaque_data.is_some() && text.is_none() {
                    "redacted_thinking"
                } else {
                    "reasoning"
                };
                Some(json!({
                    "original_type": original_type,
                    "text": text,
                    "signature": signature,
                    "opaque_data": opaque_data,
                }))
            }
            ContentBlock::RawOutput {
                original_type,
                value,
            } => Some(json!({
                "original_type": original_type,
                "value": value,
            })),
            _ => None,
        })
        .collect();
    if values.is_empty() {
        None
    } else {
        Some(values)
    }
}
```

- [ ] **Step 2: Update `convert_stored_message_to_provider_message` to load `Thinking` blocks**

In `src/chat/convert.rs`, update the raw_output loading section (lines 98-108):

```rust
if let Some(raw_output) = raw_output {
    for item in raw_output {
        if let Some(original_type) = item.get("original_type").and_then(Value::as_str) {
            match original_type {
                "thinking" | "redacted_thinking" | "reasoning" => {
                    // New format: typed fields stored directly
                    if item.get("text").is_some()
                        || item.get("signature").is_some()
                        || item.get("opaque_data").is_some()
                    {
                        content.push(ContentBlock::Thinking {
                            text: item
                                .get("text")
                                .and_then(Value::as_str)
                                .map(String::from),
                            signature: item
                                .get("signature")
                                .and_then(Value::as_str)
                                .map(String::from),
                            opaque_data: item
                                .get("opaque_data")
                                .and_then(Value::as_str)
                                .map(String::from),
                        });
                    } else if let Some(value) =
                        item.get("value").filter(|v| !v.is_null())
                    {
                        // Legacy format: extract from raw value
                        content.push(ContentBlock::Thinking {
                            text: value
                                .get("thinking")
                                .and_then(Value::as_str)
                                .map(String::from),
                            signature: value
                                .get("signature")
                                .and_then(Value::as_str)
                                .map(String::from),
                            opaque_data: value
                                .get("data")
                                .or_else(|| value.get("encrypted_content"))
                                .and_then(Value::as_str)
                                .map(String::from),
                        });
                    }
                }
                _ => {
                    let value = item.get("value").cloned().unwrap_or(Value::Null);
                    content.push(ContentBlock::RawOutput {
                        original_type: original_type.to_string(),
                        value,
                    });
                }
            }
        }
    }
}
```

- [ ] **Step 3: Update compaction token estimation**

In `src/chat/compaction.rs`, add `Thinking` to `estimate_block_tokens`:

```rust
ContentBlock::Thinking {
    text,
    signature,
    opaque_data,
} => {
    text.as_ref().map_or(0, |t| estimate_tokens(t))
        + signature.as_ref().map_or(0, |s| estimate_tokens(s))
        + opaque_data.as_ref().map_or(0, |d| estimate_tokens(d))
}
```

- [ ] **Step 4: Update compaction summary rendering**

In `src/chat/compaction.rs`, add `Thinking` to `render_messages_for_summary`:

```rust
ContentBlock::Thinking { text, .. } => {
    if let Some(text) = text {
        out.push_str(&format!("[{role} reasoning] {text}\n\n"));
    }
}
```

- [ ] **Step 5: Update transcript rendering**

In `src/chat/transcript.rs`, update the assistant branch to handle `Thinking` blocks from the new DB format. The existing code checks `original_type == "reasoning"` on raw_output items. Update to also handle `"thinking"`:

```rust
if let Some(raw_items) = msg.raw_output_parsed() {
    for item in &raw_items {
        let otype = item
            .get("original_type")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        // New format: text stored directly
        if let Some(text) = item.get("text").and_then(Value::as_str) {
            if !text.is_empty() {
                lines.push(format!("[{otype}] {}", &text[..text.len().min(200)]));
            }
        } else if otype == "reasoning" {
            // Legacy format: extract from value
            if let Some(value) = item.get("value") {
                let summary =
                    crate::providers::extract_reasoning_summary(value);
                if !summary.is_empty() {
                    lines.push(format!("[reasoning] {summary}"));
                }
            }
        }
    }
}
```

- [ ] **Step 6: Write backward-compatibility unit test for legacy DB format**

Add a test (in `src/chat/convert.rs` tests or as a standalone unit test) that verifies
old-format `raw_output` JSON blobs are correctly loaded as `ContentBlock::Thinking`:

```rust
#[test]
fn legacy_raw_output_thinking_loads_as_thinking_block() {
    let record = MessageRecord {
        id: "test".into(),
        session_id: "s".into(),
        role: "assistant".into(),
        content: String::new(),
        tool_calls: None,
        tool_results: None,
        raw_output: Some(serde_json::to_string(&json!([{
            "original_type": "thinking",
            "value": {
                "type": "thinking",
                "thinking": "let me reason about this",
                "signature": "sig_abc123"
            }
        }])).unwrap()),
        images: None,
        created_at: "2026-01-01T00:00:00Z".into(),
    };
    let msg = convert_stored_message_to_provider_message(record);
    match &msg.content[0] {
        ContentBlock::Thinking { text, signature, opaque_data } => {
            assert_eq!(text.as_deref(), Some("let me reason about this"));
            assert_eq!(signature.as_deref(), Some("sig_abc123"));
            assert!(opaque_data.is_none());
        }
        other => panic!("expected Thinking, got {other:?}"),
    }
}
```

- [ ] **Step 7: Run all tests**

Run: `cargo test`
Expected: all pass

- [ ] **Step 8: Commit**

```
feat: DB round-trip + chat layer support for ContentBlock::Thinking
```

---

## Task 7: Update live tests

**Files:**
- Modify: `tests/providers/anthropic_live.rs`
- Modify: `tests/providers/reasoning_live.rs`

- [ ] **Step 1: Update the Anthropic thinking round-trip test**

In `tests/providers/anthropic_live.rs`, update `anthropic_thinking_block_round_trip_from_typed_fields` to match on `ContentBlock::Thinking` instead of `ContentBlock::RawOutput`:

```rust
ContentBlock::Thinking {
    text,
    signature,
    ..
} => {
    let text = text.as_ref().expect("thinking text");
    let sig = signature.as_ref().expect("signature");
    assert!(!text.is_empty(), "thinking text should not be empty");
    assert!(!sig.is_empty(), "signature should not be empty");

    // Reconstruct as if from DB round-trip
    reconstructed_content.push(ContentBlock::Thinking {
        text: Some(text.clone()),
        signature: Some(sig.clone()),
        opaque_data: None,
    });
    found_thinking = true;
}
```

Also update `anthropic_multi_turn_with_history` assertions to not filter on `RawOutput` (use `Thinking` instead).

- [ ] **Step 2: Update the Codex reasoning live test**

In `tests/providers/reasoning_live.rs`, update matches from `ContentBlock::RawOutput { original_type, .. } if original_type == "reasoning"` to `ContentBlock::Thinking { .. }`.

- [ ] **Step 3: Run all live tests**

Run: `cargo test --features live-tests-llms anthropic_thinking && cargo test --features live-tests-llms reasoning`
Expected: all pass

- [ ] **Step 4: Commit**

```
test: update live tests for ContentBlock::Thinking variant
```

---

## Task 8: Fix circuit breaker — exclude 400 Bad Request

**Files:**
- Modify: `src/providers/anthropic/mod.rs:221-233`
- Modify: `src/providers/openai_oauth.rs` (equivalent catch-all)
- Modify: `src/providers/openai_compatible_provider.rs` (equivalent catch-all)

- [ ] **Step 1: Remove `record_failure` from 400 catch-all in Anthropic provider**

In `src/providers/anthropic/mod.rs`, the catch-all at line 221 fires for ANY non-success status not already handled (429, 401/403, 404, 5xx). This includes 400, which is a client error. Remove the `record_failure` call:

```rust
if !status.is_success() {
    // 400 = client error (bad request body). Don't penalize the provider.
    logfire::warn!(
        "anthropic provider non-success response",
        provider = "anthropic",
        status = status.as_u16(),
        raw_response = response_body.clone()
    );
    return Err(ProviderError::InvalidResponse(format!(
        "http status {status}: {}",
        extract_error_message(&response_body)
    )));
}
```

- [ ] **Step 2: Same fix for OpenAI OAuth provider**

Apply the same change in `src/providers/openai_oauth.rs` — remove `record_failure` from the catch-all `!status.is_success()` block.

- [ ] **Step 3: Same fix for OpenAI Compatible provider**

Apply the same change in `src/providers/openai_compatible_provider.rs`.

- [ ] **Step 4: Write a unit test for circuit breaker behavior**

In `src/providers/circuit_breaker.rs`, add a test documenting that 400-class errors should NOT open the breaker (this is enforced by callers not calling `record_failure`, but document the intent):

```rust
#[test]
fn breaker_stays_closed_without_failure_calls() {
    let cb = CircuitBreaker::default();
    // Simulate: caller gets 400 and does NOT call record_failure.
    // Breaker should remain closed.
    assert!(cb.check("model").is_none());
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test --lib providers`
Expected: all pass

- [ ] **Step 6: Commit**

```
fix: circuit breaker only triggers on transient errors, not 400 Bad Request
```

---

## Task 9: Final verification

- [ ] **Step 1: Run full CI**

Run: `just ci`
Expected: format ✓, check ✓, clippy ✓, tests ✓

- [ ] **Step 2: Run live tests**

Run: `cargo test --features live-tests-llms anthropic_thinking && cargo test --features live-tests-llms anthropic_multi_turn`
Expected: all pass

- [ ] **Step 3: Final commit if any formatting changes**

```
chore: format
```
