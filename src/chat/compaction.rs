//! Context window compaction for long conversations.
//!
//! Two-phase approach:
//! - Phase 1 (tool result masking): Replace verbose `ToolResult` blocks outside
//!   the "keep window" with compact placeholders. Free, no LLM call.
//! - Phase 2 (LLM summarization): Summarize the oldest messages into a single
//!   summary block when masking alone isn't sufficient.

use std::collections::HashMap;
use std::sync::Arc;

use crate::config::CompactionConfig;
use crate::db;
use crate::providers::types::ReasoningEffort;
use crate::providers::{
    ChatMessage, ChatRequest, ChatResponse, ContentBlock, Provider, Role, ToolDefinition,
};

// TODO: Per-language token estimation rules. The current heuristic
// (ceil(chars / 3.5)) works well for English but overestimates for
// logographic scripts and underestimates for some agglutinative languages.

// ---------------------------------------------------------------------------
// Token estimation
// ---------------------------------------------------------------------------

/// Estimate token count from text using a `ceil(chars / 3.5)` heuristic.
///
/// This intentionally overestimates (safe direction) without needing a
/// tokenizer dependency. Multi-byte UTF-8 characters inflate the byte count,
/// which acts as extra safety margin for non-Latin scripts.
#[must_use]
pub fn estimate_tokens(text: &str) -> usize {
    (text.len() as f64 / 3.5).ceil() as usize
}

/// Estimate tokens for a single [`ContentBlock`].
#[must_use]
pub fn estimate_block_tokens(block: &ContentBlock) -> usize {
    match block {
        ContentBlock::Text { text } => estimate_tokens(text),
        ContentBlock::ToolUse { id, name, input } => {
            estimate_tokens(id) + estimate_tokens(name) + estimate_tokens(&input.to_string())
        }
        ContentBlock::ToolResult {
            tool_use_id,
            content,
            ..
        } => estimate_tokens(tool_use_id) + estimate_tokens(content),
        ContentBlock::RawOutput { value, .. } => estimate_tokens(&value.to_string()),
    }
}

/// Estimate tokens for a single [`ChatMessage`] (including per-message overhead).
#[must_use]
pub fn estimate_message_tokens(message: &ChatMessage) -> usize {
    // ~4 tokens per-message overhead for role/structure
    4 + message
        .content
        .iter()
        .map(estimate_block_tokens)
        .sum::<usize>()
}

/// Estimate tokens for an entire message history.
#[must_use]
pub fn estimate_history_tokens(messages: &[ChatMessage]) -> usize {
    messages.iter().map(estimate_message_tokens).sum()
}

/// Estimate tokens consumed by tool definitions.
#[must_use]
pub fn estimate_tool_tokens(tools: &[ToolDefinition]) -> usize {
    tools
        .iter()
        .map(|t| {
            // ~20 tokens overhead for JSON schema structure
            20 + estimate_tokens(&t.name)
                + estimate_tokens(&t.description)
                + estimate_tokens(&t.input_schema.to_string())
        })
        .sum()
}

/// Token budget breakdown for a single request.
#[derive(Debug, Clone)]
pub struct TokenBudget {
    pub context_window: usize,
    pub system_tokens: usize,
    pub tool_tokens: usize,
    pub history_tokens: usize,
    pub total_estimated: usize,
    pub needs_compaction: bool,
}

/// Compute the token budget for a request.
#[tracing::instrument(skip_all, level = "debug", fields(
    context_window = context_window,
    threshold = threshold,
))]
#[must_use]
pub fn compute_budget(
    context_window: usize,
    system_prompt: &str,
    tools: &[ToolDefinition],
    history: &[ChatMessage],
    threshold: f64,
) -> TokenBudget {
    let system_tokens = estimate_tokens(system_prompt);
    let tool_tokens = estimate_tool_tokens(tools);
    let history_tokens = estimate_history_tokens(history);
    let total_estimated = system_tokens + tool_tokens + history_tokens;
    let needs_compaction = total_estimated as f64 > (context_window as f64 * threshold);

    TokenBudget {
        context_window,
        system_tokens,
        tool_tokens,
        history_tokens,
        total_estimated,
        needs_compaction,
    }
}

// ---------------------------------------------------------------------------
// Phase 1: Tool result masking
// ---------------------------------------------------------------------------

/// Build an index mapping `tool_use_id` → `tool_name` from message history.
fn build_tool_name_index(messages: &[ChatMessage]) -> HashMap<String, String> {
    let mut index = HashMap::new();
    for msg in messages {
        for block in &msg.content {
            if let ContentBlock::ToolUse { id, name, .. } = block {
                index.insert(id.clone(), name.clone());
            }
        }
    }
    index
}

/// Find a safe UTF-8 truncation point at or before `max_bytes`.
fn safe_truncate(s: &str, max_bytes: usize) -> usize {
    if max_bytes >= s.len() {
        return s.len();
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    end
}

/// Phase 1: Replace `ToolResult` blocks outside the keep window with
/// compact placeholders.
///
/// Messages at index `>= keep_start` are left untouched. Older `ToolResult`
/// blocks are replaced with `[tool_result: {name}{error} — {preview}
/// (truncated)]`.
#[tracing::instrument(skip_all, level = "debug", fields(
    total_messages = messages.len(),
    keep_start = keep_start,
    preview_chars = preview_chars,
))]
pub fn mask_tool_results(
    messages: &[ChatMessage],
    keep_start: usize,
    preview_chars: usize,
) -> Vec<ChatMessage> {
    let tool_names = build_tool_name_index(messages);

    messages
        .iter()
        .enumerate()
        .map(|(i, msg)| {
            if i >= keep_start {
                return msg.clone();
            }

            let content = msg
                .content
                .iter()
                .map(|block| match block {
                    ContentBlock::ToolResult {
                        tool_use_id,
                        content,
                        is_error,
                    } => {
                        let tool_name = tool_names
                            .get(tool_use_id)
                            .map(|s| s.as_str())
                            .unwrap_or("unknown");

                        let preview = if content.len() > preview_chars {
                            format!("{}...", &content[..safe_truncate(content, preview_chars)])
                        } else {
                            content.clone()
                        };

                        let error_tag = if *is_error { " (error)" } else { "" };

                        ContentBlock::ToolResult {
                            tool_use_id: tool_use_id.clone(),
                            content: format!(
                                "[tool_result: {tool_name}{error_tag} \
                                 — {preview} (truncated)]"
                            ),
                            is_error: *is_error,
                        }
                    }
                    other => other.clone(),
                })
                .collect();

            ChatMessage {
                role: msg.role.clone(),
                content,
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Phase 2: LLM summarization
// ---------------------------------------------------------------------------

const COMPACTION_PROMPT: &str = include_str!("../../prompts/compaction.md");

/// Result of a successful compaction.
#[derive(Debug)]
pub struct CompactionResult {
    pub summary: String,
    pub cursor_message_id: String,
}

/// Errors specific to the compaction subsystem.
#[derive(Debug, thiserror::Error)]
pub enum CompactionError {
    #[error(transparent)]
    Provider(#[from] crate::providers::ProviderError),

    #[error(transparent)]
    Database(Box<crate::db::DatabaseError>),

    #[error("LLM returned an empty summary")]
    EmptySummary,
}

impl From<crate::db::DatabaseError> for CompactionError {
    fn from(e: crate::db::DatabaseError) -> Self {
        CompactionError::Database(Box::new(e))
    }
}

/// Render messages into a plain-text format suitable for the summarization LLM.
fn render_messages_for_summary(messages: &[ChatMessage], preview_chars: usize) -> String {
    let tool_names = build_tool_name_index(messages);
    let mut out = String::new();

    for msg in messages {
        let role = match msg.role {
            Role::User => "Operator",
            Role::Assistant => "Ghost",
            Role::System => "System",
        };

        for block in &msg.content {
            match block {
                ContentBlock::Text { text } => {
                    out.push_str(&format!("[{role}] {text}\n\n"));
                }
                ContentBlock::ToolUse { name, input, .. } => {
                    out.push_str(&format!("[{role} → tool:{name}] {input}\n\n"));
                }
                ContentBlock::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                } => {
                    let tool_name = tool_names
                        .get(tool_use_id)
                        .map(|s| s.as_str())
                        .unwrap_or("unknown");
                    let tag = if *is_error { " (error)" } else { "" };
                    let preview = if content.len() > preview_chars {
                        format!(
                            "{}...(truncated)",
                            &content[..safe_truncate(content, preview_chars)]
                        )
                    } else {
                        content.clone()
                    };
                    out.push_str(&format!("[tool_result: {tool_name}{tag}] {preview}\n\n"));
                }
                ContentBlock::RawOutput { original_type, .. } => {
                    out.push_str(&format!("[{role} raw:{original_type}]\n\n"));
                }
            }
        }
    }

    out
}

/// Summarize older messages via an LLM call and persist the result.
///
/// `stored_message_ids` must be parallel to `messages` — one DB message ID
/// per provider message. The cursor is set to the last summarized message's ID.
#[tracing::instrument(skip_all, level = "debug", fields(
    total_messages = messages.len(),
    keep_window = config.keep_window,
))]
pub async fn summarize_older_messages(
    provider: &Arc<dyn Provider>,
    model: &str,
    cache_key: &str,
    messages: &[ChatMessage],
    stored_message_ids: &[String],
    config: &CompactionConfig,
) -> Result<CompactionResult, CompactionError> {
    let split = messages.len().saturating_sub(config.keep_window);
    let to_summarize = &messages[..split];
    let to_keep = &messages[split..];

    let conversation_text = render_messages_for_summary(to_summarize, config.mask_preview_chars);

    logfire::debug!(
        "Phase 2: summarizing older messages",
        messages_to_summarize = to_summarize.len() as u64,
        messages_to_keep = to_keep.len() as u64,
        chars = conversation_text.len() as u64
    );

    let response: ChatResponse = provider
        .chat(ChatRequest {
            model: model.to_string(),
            messages: vec![ChatMessage {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: conversation_text,
                }],
            }],
            tools: None,
            max_tokens: Some(2048),
            temperature: Some(0.3),
            system: Some(COMPACTION_PROMPT.to_string()),
            reasoning_effort: Some(ReasoningEffort::Low),
            cache_key: cache_key.to_string(),
            debug_context: None,
        })
        .await?;

    let summary = response
        .content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n\n");

    if summary.trim().is_empty() {
        return Err(CompactionError::EmptySummary);
    }

    let cursor_id = stored_message_ids
        .get(split.saturating_sub(1))
        .cloned()
        .unwrap_or_default();

    logfire::debug!(
        "Phase 2 complete",
        compacted_count = to_summarize.len() as u64,
        summary_len = summary.len() as u64,
        cursor_id = cursor_id.clone()
    );

    Ok(CompactionResult {
        summary,
        cursor_message_id: cursor_id,
    })
}

// ---------------------------------------------------------------------------
// Integration: compact_if_needed on SessionChat
// ---------------------------------------------------------------------------

use super::session::SessionChat;

impl SessionChat {
    /// Check token budget and run compaction if needed.
    ///
    /// Phase 1 (tool result masking) is tried first. If that isn't enough,
    /// Phase 2 (LLM summarization) kicks in. Provider or empty-summary errors
    /// are logged and gracefully degraded — they never fail the chat.
    #[tracing::instrument(skip_all, level = "debug", fields(session_id = ?session_id))]
    pub(super) async fn compact_if_needed(
        &self,
        session_id: &str,
        history: &mut Vec<ChatMessage>,
        stored_message_ids: &[String],
    ) {
        let model_name = match self.default_model_name() {
            Ok(m) => m,
            Err(_) => return,
        };

        let alias = &self.config().models.default;
        let context_window = self
            .config()
            .models
            .aliases
            .get(alias)
            .map(|m| m.context_window as usize)
            .unwrap_or(200_000);

        let compaction = &self.config().compaction;
        let tools = self.tool_manager().all_tool_schemas();
        let system_prompt = String::new(); // conservative: ignore system tokens

        let budget = compute_budget(
            context_window,
            &system_prompt,
            &tools,
            history,
            compaction.threshold,
        );

        if !budget.needs_compaction {
            return;
        }

        logfire::info!(
            "Compaction triggered",
            total = budget.total_estimated as u64,
            window = budget.context_window as u64,
            history = budget.history_tokens as u64
        );

        // Phase 1: mask tool results
        let keep_start = history.len().saturating_sub(compaction.keep_window);
        let masked = mask_tool_results(history, keep_start, compaction.mask_preview_chars);
        let masked_tokens = estimate_history_tokens(&masked);

        logfire::debug!(
            "Phase 1: observation masking complete",
            before = budget.history_tokens as u64,
            after = masked_tokens as u64,
            saved = budget.history_tokens.saturating_sub(masked_tokens) as u64
        );

        let total_after_mask = budget.system_tokens + budget.tool_tokens + masked_tokens;
        let still_over =
            total_after_mask as f64 > (budget.context_window as f64 * compaction.threshold);

        if !still_over {
            *history = masked;
            return;
        }

        // Phase 2: LLM summarization
        logfire::info!("Masking insufficient — proceeding to Phase 2");

        let cache_key = session_id.to_string();
        match summarize_older_messages(
            self.provider(),
            &model_name,
            &cache_key,
            &masked,
            stored_message_ids,
            compaction,
        )
        .await
        {
            Ok(result) => {
                if let Err(e) = db::sessions::update_compaction(
                    self.db(),
                    session_id,
                    &result.summary,
                    &result.cursor_message_id,
                )
                .await
                {
                    logfire::error!(
                        "Failed to persist compaction summary",
                        error = e.to_string()
                    );
                    *history = masked;
                    return;
                }

                // Reload history from DB to reflect compaction
                match self.load_provider_history(session_id).await {
                    Ok((reloaded, _ids)) => *history = reloaded,
                    Err(e) => {
                        logfire::error!(
                            "Failed to reload history after compaction",
                            error = e.to_string()
                        );
                        *history = masked;
                    }
                }
            }
            Err(e) => {
                logfire::warn!(
                    "Phase 2 summarization failed — using masked history",
                    error = e.to_string()
                );
                *history = masked;
            }
        }
    }

    /// Lightweight Phase 1 masking during tool loops (no LLM call).
    #[tracing::instrument(skip_all, level = "debug")]
    pub(super) fn apply_masking_if_needed(&self, history: &mut Vec<ChatMessage>) {
        let alias = &self.config().models.default;
        let context_window = self
            .config()
            .models
            .aliases
            .get(alias)
            .map(|m| m.context_window as usize)
            .unwrap_or(200_000);

        let compaction = &self.config().compaction;
        let tools = self.tool_manager().all_tool_schemas();

        let budget = compute_budget(context_window, "", &tools, history, compaction.threshold);

        if !budget.needs_compaction {
            return;
        }

        let keep_start = history.len().saturating_sub(compaction.keep_window);
        *history = mask_tool_results(history, keep_start, compaction.mask_preview_chars);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn user_text(text: &str) -> ChatMessage {
        ChatMessage {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: text.to_string(),
            }],
        }
    }

    fn assistant_text(text: &str) -> ChatMessage {
        ChatMessage {
            role: Role::Assistant,
            content: vec![ContentBlock::Text {
                text: text.to_string(),
            }],
        }
    }

    fn assistant_with_tool(text: &str, tool_id: &str, tool_name: &str) -> ChatMessage {
        ChatMessage {
            role: Role::Assistant,
            content: vec![
                ContentBlock::Text {
                    text: text.to_string(),
                },
                ContentBlock::ToolUse {
                    id: tool_id.to_string(),
                    name: tool_name.to_string(),
                    input: json!({"query": "test"}),
                },
            ],
        }
    }

    fn tool_result(tool_id: &str, content: &str) -> ChatMessage {
        ChatMessage {
            role: Role::User,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: tool_id.to_string(),
                content: content.to_string(),
                is_error: false,
            }],
        }
    }

    fn tool_result_error(tool_id: &str, content: &str) -> ChatMessage {
        ChatMessage {
            role: Role::User,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: tool_id.to_string(),
                content: content.to_string(),
                is_error: true,
            }],
        }
    }

    // -----------------------------------------------------------------------
    // Token estimation
    // -----------------------------------------------------------------------

    #[test]
    fn estimate_tokens_basic() {
        assert_eq!(estimate_tokens("hello!!"), 2); // 7 / 3.5 = 2
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens(&"a".repeat(35)), 10);
    }

    #[test]
    fn estimate_tokens_unicode() {
        let jp = "こんにちは"; // 15 bytes in UTF-8
        let tokens = estimate_tokens(jp);
        assert!(tokens >= 4); // 15 / 3.5 ≈ 4.3
    }

    #[test]
    fn estimate_history_includes_overhead() {
        let history = vec![user_text("Hello")];
        let tokens = estimate_history_tokens(&history);
        // "Hello" = 5 chars → ceil(5/3.5) = 2, plus 4 overhead = 6
        assert_eq!(tokens, 6);
    }

    #[test]
    fn estimate_tool_tokens_includes_schema_overhead() {
        let tools = vec![ToolDefinition {
            name: "echo".to_string(),
            description: "echoes".to_string(),
            input_schema: json!({"type": "object"}),
        }];
        let tokens = estimate_tool_tokens(&tools);
        assert!(tokens > 20); // at least the 20-token overhead
    }

    #[test]
    fn compute_budget_no_compaction() {
        let budget = compute_budget(200_000, "System prompt", &[], &[user_text("Hello")], 0.85);
        assert!(!budget.needs_compaction);
        assert!(budget.context_window > budget.total_estimated);
    }

    #[test]
    fn compute_budget_triggers_compaction() {
        let big = user_text(&"x".repeat(700_000)); // ~200K tokens
        let budget = compute_budget(200_000, "System", &[], &[big], 0.85);
        assert!(budget.needs_compaction);
    }

    // -----------------------------------------------------------------------
    // Phase 1: tool result masking
    // -----------------------------------------------------------------------

    #[test]
    fn mask_preserves_keep_window() {
        let messages = vec![
            user_text("Hello"),
            assistant_with_tool("Let me search", "tu_1", "web_search"),
            tool_result("tu_1", &"x".repeat(500)),
            user_text("Thanks"),
        ];

        // keep_start=0 means all messages are at index >= keep_start, so
        // none get masked.
        let masked = mask_tool_results(&messages, 0, 100);
        if let ContentBlock::ToolResult { content, .. } = &masked[2].content[0] {
            assert_eq!(content.len(), 500); // unchanged
        } else {
            panic!("expected tool result");
        }
    }

    #[test]
    fn mask_truncates_old_tool_results() {
        let messages = vec![
            user_text("Hello"),
            assistant_with_tool("Searching", "tu_1", "web_search"),
            tool_result("tu_1", &"a".repeat(500)),
            user_text("OK"),
            assistant_with_tool("Fetching", "tu_2", "web_fetch"),
            tool_result("tu_2", &"b".repeat(300)),
            user_text("Final question"),
        ];

        // keep last 3 messages (indices 4,5,6)
        let keep_start = messages.len() - 3;
        let masked = mask_tool_results(&messages, keep_start, 50);

        // tu_1 result (index 2) should be masked
        if let ContentBlock::ToolResult { content, .. } = &masked[2].content[0] {
            assert!(content.starts_with("[tool_result: web_search"));
            assert!(content.contains("(truncated)"));
            assert!(content.len() < 200);
        } else {
            panic!("expected tool result at index 2");
        }

        // tu_2 result (index 5) is within keep window — not masked
        if let ContentBlock::ToolResult { content, .. } = &masked[5].content[0] {
            assert_eq!(content.len(), 300); // unchanged
        } else {
            panic!("expected tool result at index 5");
        }
    }

    #[test]
    fn mask_resolves_tool_names() {
        let messages = vec![
            assistant_with_tool("Checking", "tu_abc", "knowledge_search"),
            tool_result("tu_abc", &"result ".repeat(100)),
            user_text("Done"),
        ];

        let keep_start = messages.len() - 1;
        let masked = mask_tool_results(&messages, keep_start, 20);
        if let ContentBlock::ToolResult { content, .. } = &masked[1].content[0] {
            assert!(content.contains("knowledge_search"));
        } else {
            panic!("expected tool result");
        }
    }

    #[test]
    fn mask_handles_error_results() {
        let messages = vec![
            assistant_with_tool("Trying", "tu_1", "shell"),
            tool_result_error("tu_1", "Permission denied: /etc/shadow"),
            user_text("That failed"),
        ];

        let keep_start = messages.len() - 1;
        let masked = mask_tool_results(&messages, keep_start, 100);
        if let ContentBlock::ToolResult { content, .. } = &masked[1].content[0] {
            assert!(content.contains("(error)"));
            assert!(content.contains("shell"));
        } else {
            panic!("expected tool result");
        }
    }

    #[test]
    fn mask_short_content_not_truncated() {
        let messages = vec![
            assistant_with_tool("Check", "tu_1", "shell"),
            tool_result("tu_1", "OK"),
            user_text("Great"),
        ];

        let keep_start = messages.len() - 1;
        let masked = mask_tool_results(&messages, keep_start, 100);
        if let ContentBlock::ToolResult { content, .. } = &masked[1].content[0] {
            assert!(content.contains("OK"));
            assert!(content.contains("shell"));
        } else {
            panic!("expected tool result");
        }
    }

    #[test]
    fn safe_truncate_unicode() {
        let s = "こんにちは";
        assert_eq!(safe_truncate(s, 100), 15);
        assert_eq!(safe_truncate(s, 6), 6);
        assert_eq!(safe_truncate(s, 7), 6);
        assert_eq!(safe_truncate(s, 0), 0);
    }

    // -----------------------------------------------------------------------
    // Phase 2: render for summary
    // -----------------------------------------------------------------------

    #[test]
    fn render_messages_for_summary_basic() {
        let messages = vec![
            user_text("Hello, can you help?"),
            assistant_text("Of course!"),
            assistant_with_tool("Let me check", "tu_1", "shell"),
            tool_result("tu_1", "file1.txt\nfile2.txt"),
        ];

        let rendered = render_messages_for_summary(&messages, 500);
        assert!(rendered.contains("[Operator] Hello, can you help?"));
        assert!(rendered.contains("[Ghost] Of course!"));
        assert!(rendered.contains("[Ghost → tool:shell]"));
        assert!(rendered.contains("[tool_result: shell]"));
    }

    #[test]
    fn render_messages_truncates_long_tool_results() {
        let messages = vec![
            assistant_with_tool("Check", "tu_1", "shell"),
            tool_result("tu_1", &"x".repeat(1000)),
        ];

        let rendered = render_messages_for_summary(&messages, 500);
        assert!(rendered.contains("...(truncated)"));
        assert!(rendered.len() < 1000);
    }
}
