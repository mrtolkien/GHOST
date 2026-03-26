//! Context window compaction for long conversations.
//!
//! Two-phase approach:
//! - Phase 1 (tool interaction masking): Replace verbose `ToolResult` and
//!   `ToolUse` blocks before the current turn with compact placeholders.
//!   Free, no LLM call.
//! - Phase 2 (LLM summarization): Summarize the masked pre-turn messages
//!   into a single summary block when masking alone isn't sufficient.

use std::collections::HashMap;
use std::sync::Arc;

use crate::config::CompactionConfig;
use crate::db;
use crate::providers::types::ReasoningEffort;
use crate::providers::{
    ChatMessage, ChatRequest, ChatResponse, ContentBlock, Provider, Role, ToolDefinition,
};

// ---------------------------------------------------------------------------
// Token estimation
// ---------------------------------------------------------------------------

/// Estimate token count from text using a `ceil(bytes / 4)` heuristic.
///
/// Industry-standard ratio (used by Claude Code, Codex CLI). Multi-byte
/// UTF-8 characters inflate the byte count, providing extra safety margin
/// for non-Latin scripts.
#[must_use]
pub fn estimate_tokens(text: &str) -> usize {
    text.len().div_ceil(4)
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
        ContentBlock::Image { .. } => 1000, // rough estimate for a compressed image
        ContentBlock::Thinking {
            text,
            signature,
            opaque_data,
        } => {
            text.as_ref().map_or(0, |t| estimate_tokens(t))
                + signature.as_ref().map_or(0, |s| estimate_tokens(s))
                + opaque_data.as_ref().map_or(0, |d| estimate_tokens(d))
        }
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
// Current-turn boundary
// ---------------------------------------------------------------------------

/// Find the index of the last user message that contains actual text
/// (not just tool results). Everything from this index onward is the
/// "current turn" and should be preserved verbatim during compaction.
#[must_use]
pub fn find_current_turn_start(messages: &[ChatMessage]) -> usize {
    for (i, msg) in messages.iter().enumerate().rev() {
        if msg.role != Role::User {
            continue;
        }
        let has_text = msg
            .content
            .iter()
            .any(|block| matches!(block, ContentBlock::Text { text } if !text.is_empty()));
        if has_text {
            return i;
        }
    }
    0
}

// ---------------------------------------------------------------------------
// Phase 1: Tool interaction masking
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

/// Phase 1: Replace `ToolUse` inputs and `ToolResult` blocks outside the keep
/// window with compact placeholders.
///
/// Messages at index `>= keep_start` are left untouched. Older `ToolUse`
/// blocks have their `input` replaced with `{}`. Older `ToolResult` blocks
/// are replaced with `[tool_result: {name}{error} — {preview} (truncated)]`.
#[tracing::instrument(skip_all, level = "debug", fields(
    total_messages = messages.len(),
    keep_start = keep_start,
    preview_chars = preview_chars,
))]
pub fn mask_tool_interactions(
    messages: &[ChatMessage],
    keep_start: usize,
    preview_chars: usize,
) -> Vec<ChatMessage> {
    let no_compacted = vec![false; messages.len()];
    mask_tool_interactions_with_compacted(messages, keep_start, preview_chars, &no_compacted)
}

/// Phase 1 masking that skips already-compacted messages.
///
/// Messages where `compacted[i]` is `true` are cloned as-is (their content
/// was already masked and persisted in a previous compaction run).
/// Messages at index `>= keep_start` are also cloned as-is (current turn).
#[tracing::instrument(skip_all, level = "debug", fields(
    total_messages = messages.len(),
    keep_start = keep_start,
    preview_chars = preview_chars,
))]
pub fn mask_tool_interactions_with_compacted(
    messages: &[ChatMessage],
    keep_start: usize,
    preview_chars: usize,
    compacted: &[bool],
) -> Vec<ChatMessage> {
    let tool_names = build_tool_name_index(messages);

    messages
        .iter()
        .enumerate()
        .map(|(i, msg)| {
            // Already compacted or in the current turn — pass through
            if i >= keep_start || compacted.get(i).copied().unwrap_or(false) {
                return msg.clone();
            }

            let content = msg
                .content
                .iter()
                .map(|block| match block {
                    ContentBlock::ToolUse { id, name, .. } => ContentBlock::ToolUse {
                        id: id.clone(),
                        name: name.clone(),
                        input: serde_json::json!({}),
                    },
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
                    ContentBlock::Image { filename, .. } => ContentBlock::Text {
                        text: format!("[image: {filename}]"),
                    },
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
// JSON serialization helpers for persisting masked content
// ---------------------------------------------------------------------------

/// Serialize tool call blocks to JSON string (same format as DB storage).
fn tool_calls_to_json(content: &[ContentBlock]) -> Option<String> {
    let calls: Vec<serde_json::Value> = content
        .iter()
        .filter_map(|b| match b {
            ContentBlock::ToolUse { id, name, input } => Some(serde_json::json!({
                "id": id,
                "name": name,
                "input": input,
            })),
            _ => None,
        })
        .collect();
    if calls.is_empty() {
        None
    } else {
        match serde_json::to_string(&calls) {
            Ok(json) => Some(json),
            Err(e) => {
                tracing::warn!(error = %e, "Failed to serialize tool calls for compaction");
                None
            }
        }
    }
}

/// Serialize tool result blocks to JSON string (same format as DB storage).
fn tool_results_to_json(content: &[ContentBlock]) -> Option<String> {
    let results: Vec<serde_json::Value> = content
        .iter()
        .filter_map(|b| match b {
            ContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => Some(serde_json::json!({
                "tool_use_id": tool_use_id,
                "content": content,
                "is_error": is_error,
            })),
            _ => None,
        })
        .collect();
    if results.is_empty() {
        None
    } else {
        match serde_json::to_string(&results) {
            Ok(json) => Some(json),
            Err(e) => {
                tracing::warn!(error = %e, "Failed to serialize tool results for compaction");
                None
            }
        }
    }
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

    #[error("cursor mismatch: split={split} but stored_message_ids has {ids_len} entries")]
    CursorMismatch { split: usize, ids_len: usize },
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
                ContentBlock::Image { filename, .. } => {
                    out.push_str(&format!("[image: {filename}]\n\n"));
                }
                ContentBlock::Thinking { text, .. } => {
                    if let Some(text) = text {
                        out.push_str(&format!("[{role} reasoning] {text}\n\n"));
                    }
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
///
/// When `instructions` is provided, it is appended to the base compaction
/// prompt as an "Additional instructions" section.
#[tracing::instrument(skip_all, level = "debug", fields(
    total_messages = messages.len(),
))]
pub async fn summarize_older_messages(
    provider: &Arc<dyn Provider>,
    model: &str,
    cache_key: &str,
    messages: &[ChatMessage],
    stored_message_ids: &[String],
    config: &CompactionConfig,
    instructions: Option<&str>,
) -> Result<CompactionResult, CompactionError> {
    let split = find_current_turn_start(messages);
    let to_summarize = &messages[..split];
    let to_keep = &messages[split..];

    let conversation_text = render_messages_for_summary(to_summarize, config.mask_preview_chars);

    const MAX_SUMMARIZATION_INPUT_CHARS: usize = 50_000;

    let conversation_text = if conversation_text.len() > MAX_SUMMARIZATION_INPUT_CHARS {
        let mut start = conversation_text.len() - MAX_SUMMARIZATION_INPUT_CHARS;
        while start < conversation_text.len() && !conversation_text.is_char_boundary(start) {
            start += 1;
        }
        format!(
            "[earlier conversation truncated]\n\n{}",
            &conversation_text[start..]
        )
    } else {
        conversation_text
    };

    tracing::debug!(
        messages_to_summarize = to_summarize.len() as u64,
        messages_to_keep = to_keep.len() as u64,
        chars = conversation_text.len() as u64,
        "Phase 2: summarizing older messages",
    );

    let system = match instructions {
        Some(extra) => format!("{COMPACTION_PROMPT}\n\n## Additional instructions\n\n{extra}"),
        None => COMPACTION_PROMPT.to_string(),
    };

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
            system: Some(system),
            reasoning_effort: Some(ReasoningEffort::Low),
            cache_key: cache_key.to_string(),
            turn_state: None,
            debug_context: None,
            text_verbosity: None,
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

    let cursor_idx = split.saturating_sub(1);
    let cursor_id = stored_message_ids
        .get(cursor_idx)
        .filter(|id| !id.is_empty())
        .cloned()
        .ok_or(CompactionError::CursorMismatch {
            split,
            ids_len: stored_message_ids.len(),
        })?;

    tracing::debug!(
        compacted_count = to_summarize.len() as u64,
        summary_len = summary.len() as u64,
        cursor_id = cursor_id.clone(),
        "Phase 2 complete",
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
    /// Called at the start of a chat turn with pre-loaded IDs that are
    /// guaranteed parallel to `history`.
    #[tracing::instrument(skip_all, level = "debug", fields(session_id = ?session_id))]
    pub(super) async fn compact_if_needed(
        &self,
        session_id: &str,
        history: &mut Vec<ChatMessage>,
        stored_message_ids: &[String],
        compacted_flags: &[bool],
    ) {
        let compaction = self.compaction_config();
        self.run_compaction(
            session_id,
            history,
            stored_message_ids,
            compacted_flags,
            &compaction,
        )
        .await;
    }

    /// Compaction for use during tool loops (default config).
    #[tracing::instrument(skip_all, level = "debug", fields(session_id = %session_id))]
    pub(super) async fn compact_in_tool_loop(
        &self,
        session_id: &str,
        history: &mut Vec<ChatMessage>,
    ) -> bool {
        let compaction = self.compaction_config();
        self.compact_in_tool_loop_with_config(session_id, history, &compaction)
            .await
    }

    /// Compaction for use during tool loops with an explicit config.
    ///
    /// Loads stored message IDs and compacted flags from DB and verifies
    /// they match the in-memory history length. Falls back to Phase 1
    /// masking only when IDs don't match (prevents the empty-cursor bug).
    ///
    /// Returns `true` when Phase 2 summarization ran successfully.
    #[tracing::instrument(skip_all, level = "debug", fields(session_id = %session_id))]
    pub(super) async fn compact_in_tool_loop_with_config(
        &self,
        session_id: &str,
        history: &mut Vec<ChatMessage>,
        compaction: &CompactionConfig,
    ) -> bool {
        // Load IDs and compacted flags from DB — messages were persisted
        // before this call.
        let id_flag_pairs =
            match db::sessions::get_session_message_ids_and_compacted(self.db(), session_id).await {
                Ok(pairs) => pairs,
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to load message IDs for compaction");
                    return false;
                }
            };

        // Build parallel IDs and compacted flags matching the in-memory
        // history structure.
        let session = match db::sessions::get_session(self.db(), session_id).await {
            Ok(s) => s,
            Err(_) => return false,
        };

        let mut parallel_ids = Vec::with_capacity(history.len());
        let mut compacted_flags = Vec::with_capacity(history.len());
        if session.compaction_summary.is_some() {
            parallel_ids.push(String::new());
            compacted_flags.push(false);
        }
        let cursor = session.compaction_cursor_id.filter(|c| !c.is_empty());
        let mut include = cursor.is_none();
        for (id, compacted) in &id_flag_pairs {
            if !include {
                include = Some(id.clone()) == cursor;
                continue;
            }
            parallel_ids.push(id.clone());
            compacted_flags.push(*compacted);
        }

        // Safety check: if IDs don't match history, log and fall back to
        // Phase 1 masking only — never attempt Phase 2 with mismatched IDs.
        if parallel_ids.len() != history.len() {
            tracing::error!(
                history_len = history.len(),
                ids_len = parallel_ids.len(),
                "compaction ID mismatch — falling back to Phase 1 masking only",
            );
            let keep_start = find_current_turn_start(history);
            *history = mask_tool_interactions(history, keep_start, compaction.mask_preview_chars);
            return false;
        }

        self.run_compaction(
            session_id,
            history,
            &parallel_ids,
            &compacted_flags,
            compaction,
        )
        .await
    }

    /// Shared compaction logic for both pre-request and tool-loop paths.
    ///
    /// `stored_message_ids` and `compacted_flags` must be parallel to
    /// `history` — one entry per provider message. Callers are responsible
    /// for providing correct arrays.
    ///
    /// Phase 1 (tool result masking) is tried first. If that isn't enough,
    /// Phase 2 (LLM summarization) kicks in. Provider or empty-summary
    /// errors are logged and gracefully degraded — they never fail the chat.
    ///
    /// Returns `true` when Phase 2 ran successfully.
    async fn run_compaction(
        &self,
        session_id: &str,
        history: &mut Vec<ChatMessage>,
        stored_message_ids: &[String],
        compacted_flags: &[bool],
        compaction: &CompactionConfig,
    ) -> bool {
        let context_window = self.model_context_window();
        let tools = self.tool_manager().all_tool_schemas();

        let budget = compute_budget(context_window, "", &tools, history, compaction.threshold);

        if !budget.needs_compaction {
            return false;
        }

        tracing::info!(
            total = budget.total_estimated as u64,
            window = budget.context_window as u64,
            history = budget.history_tokens as u64,
            "Compaction triggered",
        );

        // Phase 1: mask tool interactions (skipping already-compacted messages)
        let keep_start = find_current_turn_start(history);
        let masked = mask_tool_interactions_with_compacted(
            history,
            keep_start,
            compaction.mask_preview_chars,
            compacted_flags,
        );
        let masked_tokens = estimate_history_tokens(&masked);

        tracing::debug!(
            before = budget.history_tokens as u64,
            after = masked_tokens as u64,
            saved = budget.history_tokens.saturating_sub(masked_tokens) as u64,
            "Phase 1: observation masking complete",
        );

        // Persist newly-masked messages to DB
        self.persist_masked_messages(
            &masked,
            history,
            keep_start,
            stored_message_ids,
            compacted_flags,
        )
        .await;

        let total_after_mask = budget.system_tokens + budget.tool_tokens + masked_tokens;
        let still_over =
            total_after_mask as f64 > (budget.context_window as f64 * compaction.threshold);

        if !still_over {
            *history = masked;
            return false;
        }

        // Phase 2: LLM summarization
        tracing::info!("Masking insufficient — proceeding to Phase 2");

        let model_name = match self.default_model_name() {
            Ok(m) => m,
            Err(_) => {
                *history = masked;
                return false;
            }
        };

        let cache_key = session_id.to_string();
        match summarize_older_messages(
            self.provider(),
            &model_name,
            &cache_key,
            &masked,
            stored_message_ids,
            compaction,
            compaction.instructions.as_deref(),
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
                    tracing::error!(
                        error = e.to_string(),
                        "Failed to persist compaction summary",
                    );
                    *history = masked;
                    return false;
                }

                match self.load_provider_history(session_id).await {
                    Ok((reloaded, _ids, _flags)) => *history = reloaded,
                    Err(e) => {
                        tracing::error!(
                            error = e.to_string(),
                            "Failed to reload history after compaction",
                        );
                        *history = masked;
                    }
                }
                true
            }
            Err(e) => {
                tracing::warn!(
                    error = e.to_string(),
                    "Phase 2 summarization failed — using masked history",
                );
                *history = masked;
                false
            }
        }
    }

    /// Persist Phase 1 masking results to DB for messages that were newly masked.
    ///
    /// Skips messages in the current turn (`>= keep_start`), already-compacted
    /// messages, and messages without tool content.
    async fn persist_masked_messages(
        &self,
        masked: &[ChatMessage],
        original: &[ChatMessage],
        keep_start: usize,
        stored_message_ids: &[String],
        compacted_flags: &[bool],
    ) {
        for (i, (masked_msg, original_msg)) in masked.iter().zip(original.iter()).enumerate() {
            // Current turn or already persisted — skip
            if i >= keep_start || compacted_flags.get(i).copied().unwrap_or(false) {
                continue;
            }

            let has_tool_content = original_msg.content.iter().any(|b| {
                matches!(
                    b,
                    ContentBlock::ToolUse { .. } | ContentBlock::ToolResult { .. }
                )
            });
            if !has_tool_content {
                continue;
            }

            let msg_id = match stored_message_ids.get(i) {
                Some(id) if !id.is_empty() => id,
                _ => continue, // Summary pseudo-message or out-of-bounds
            };

            let masked_tool_calls = tool_calls_to_json(&masked_msg.content);
            let masked_tool_results = tool_results_to_json(&masked_msg.content);

            if let Err(e) = db::sessions::update_message_compacted(
                self.db(),
                msg_id,
                masked_tool_calls.as_deref(),
                masked_tool_results.as_deref(),
            )
            .await
            {
                tracing::warn!(
                    error = %e,
                    message_id = msg_id,
                    "Failed to persist compacted message",
                );
            }
        }
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
        assert_eq!(estimate_tokens("hello!!"), 2); // ceil(7 / 4) = 2
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens(&"a".repeat(32)), 8); // 32 / 4 = 8
        assert_eq!(estimate_tokens(&"a".repeat(33)), 9); // ceil(33 / 4) = 9
    }

    #[test]
    fn estimate_tokens_unicode() {
        let jp = "こんにちは"; // 15 bytes in UTF-8
        let tokens = estimate_tokens(jp);
        assert_eq!(tokens, 4); // ceil(15 / 4) = 4 (overestimates for CJK — safe)
    }

    #[test]
    fn estimate_history_includes_overhead() {
        let history = vec![user_text("Hello")];
        let tokens = estimate_history_tokens(&history);
        // "Hello" = 5 bytes → ceil(5/4) = 2, plus 4 overhead = 6
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
        let budget = compute_budget(200_000, "System prompt", &[], &[user_text("Hello")], 0.90);
        assert!(!budget.needs_compaction);
        assert!(budget.context_window > budget.total_estimated);
    }

    #[test]
    fn compute_budget_triggers_compaction() {
        let big = user_text(&"x".repeat(800_000)); // ~200K tokens at /4
        let budget = compute_budget(200_000, "System", &[], &[big], 0.90);
        assert!(budget.needs_compaction);
    }

    #[test]
    fn compute_budget_threshold_boundary() {
        // Build a history that uses exactly 90_000 tokens against a 100K window.
        // estimate_tokens("x".repeat(n)) = ceil(n / 4), plus 4 per-message overhead.
        // We want total = 90_000. With one message: total = ceil(bytes / 4) + 4.
        // Solve: ceil(bytes / 4) = 89_996 → bytes = 89_996 * 4 = 359_984.
        let bytes = 359_984;
        let msg = user_text(&"x".repeat(bytes));
        let total = estimate_history_tokens(std::slice::from_ref(&msg));
        assert_eq!(total, 90_000, "sanity: estimated tokens should be 90_000");

        // At 0.90 threshold → limit is 90_000. 90_000 > 90_000 is false,
        // so add 4 more bytes (+1 token) to go strictly over.
        let msg_over = user_text(&"x".repeat(bytes + 4));
        let budget_at_90 = compute_budget(100_000, "", &[], std::slice::from_ref(&msg_over), 0.90);
        assert!(
            budget_at_90.needs_compaction,
            "should trigger at 0.90 threshold"
        );

        // At threshold 0.91 → limit is 91_000 → does not trigger
        let budget_at_91 = compute_budget(100_000, "", &[], &[msg_over], 0.91);
        assert!(
            !budget_at_91.needs_compaction,
            "should NOT trigger at 0.91 threshold"
        );
    }

    // -----------------------------------------------------------------------
    // Current-turn boundary
    // -----------------------------------------------------------------------

    #[test]
    fn find_current_turn_start_after_last_user_text() {
        let messages = vec![
            user_text("Hello"),                                     // 0
            assistant_text("Hi"),                                   // 1
            user_text("Search for X"),                              // 2
            assistant_with_tool("Searching", "tu_1", "web_search"), // 3
            tool_result("tu_1", "results..."),                      // 4
            assistant_with_tool("Fetching", "tu_2", "web_fetch"),   // 5
            tool_result("tu_2", "page content..."),                 // 6
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
            user_text("Do something"),
            assistant_with_tool("OK", "tu_1", "shell"),
            tool_result("tu_1", "output"),
            assistant_with_tool("More", "tu_2", "shell"),
            tool_result("tu_2", "output2"),
        ];
        assert_eq!(find_current_turn_start(&messages), 0);
    }

    // -----------------------------------------------------------------------
    // Phase 1: tool interaction masking
    // -----------------------------------------------------------------------

    #[test]
    fn mask_preserves_current_turn() {
        let messages = vec![
            user_text("Hello"),
            assistant_with_tool("Let me search", "tu_1", "web_search"),
            tool_result("tu_1", &"x".repeat(500)),
            user_text("Thanks"),
        ];

        // keep_start=0 means all messages are at index >= keep_start, so
        // none get masked.
        let masked = mask_tool_interactions(&messages, 0, 100);
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
        let masked = mask_tool_interactions(&messages, keep_start, 50);

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
        let masked = mask_tool_interactions(&messages, keep_start, 20);
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
        let masked = mask_tool_interactions(&messages, keep_start, 100);
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
        let masked = mask_tool_interactions(&messages, keep_start, 100);
        if let ContentBlock::ToolResult { content, .. } = &masked[1].content[0] {
            assert!(content.contains("OK"));
            assert!(content.contains("shell"));
        } else {
            panic!("expected tool result");
        }
    }

    #[test]
    fn mask_includes_tool_use_inputs() {
        let messages = vec![
            user_text("Hello"),
            assistant_with_tool("Let me search", "tu_1", "web_search"),
            tool_result("tu_1", &"x".repeat(500)),
            user_text("Thanks"),
        ];
        let masked = mask_tool_interactions(&messages, 3, 100);
        // Tool result masked
        if let ContentBlock::ToolResult { content, .. } = &masked[2].content[0] {
            assert!(content.contains("[tool_result:"));
        } else {
            panic!("expected tool result");
        }
        // Tool use input replaced with {}
        if let ContentBlock::ToolUse { input, .. } = &masked[1].content[1] {
            assert_eq!(input.to_string(), "{}");
        } else {
            panic!("expected tool use at index 1 content 1");
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

    // -----------------------------------------------------------------------
    // Phase 1: compacted flag support
    // -----------------------------------------------------------------------

    #[test]
    fn mask_skips_already_compacted_messages() {
        let already_masked = "[tool_result: web_fetch — https://example.com... (truncated)]";
        let messages = vec![
            user_text("Hello"),
            assistant_with_tool("Searching", "tu_1", "web_fetch"),
            ChatMessage {
                role: Role::User,
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "tu_1".to_string(),
                    content: already_masked.to_string(),
                    is_error: false,
                }],
            },
            user_text("Thanks"),
        ];

        let compacted_flags = vec![false, false, true, false];
        let masked =
            mask_tool_interactions_with_compacted(&messages, 3, 100, &compacted_flags);

        // The already-compacted tool result at index 2 should be passed
        // through unchanged.
        if let ContentBlock::ToolResult { content, .. } = &masked[2].content[0] {
            assert_eq!(content, already_masked);
        } else {
            panic!("expected tool result");
        }
    }

    #[test]
    fn mask_with_compacted_still_masks_non_compacted() {
        let messages = vec![
            user_text("Hello"),
            assistant_with_tool("First", "tu_1", "web_search"),
            tool_result("tu_1", &"a".repeat(500)),
            assistant_with_tool("Second", "tu_2", "web_fetch"),
            tool_result("tu_2", &"b".repeat(500)),
            user_text("Done"),
        ];

        // Only index 2 (first tool result) is already compacted
        let compacted_flags = vec![false, false, true, false, false, false];
        let masked =
            mask_tool_interactions_with_compacted(&messages, 5, 50, &compacted_flags);

        // Index 2 should be passed through (compacted)
        if let ContentBlock::ToolResult { content, .. } = &masked[2].content[0] {
            assert_eq!(content.len(), 500, "compacted message should be unchanged");
        } else {
            panic!("expected tool result at index 2");
        }

        // Index 4 should be masked (not compacted, before keep_start)
        if let ContentBlock::ToolResult { content, .. } = &masked[4].content[0] {
            assert!(
                content.starts_with("[tool_result:"),
                "non-compacted message should be masked"
            );
        } else {
            panic!("expected tool result at index 4");
        }
    }

    // -----------------------------------------------------------------------
    // JSON serialization helpers
    // -----------------------------------------------------------------------

    #[test]
    fn tool_calls_to_json_roundtrip() {
        let content = vec![
            ContentBlock::Text {
                text: "Let me check".to_string(),
            },
            ContentBlock::ToolUse {
                id: "tu_1".to_string(),
                name: "shell".to_string(),
                input: json!({}),
            },
        ];
        let json = tool_calls_to_json(&content).unwrap();
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0]["id"], "tu_1");
        assert_eq!(parsed[0]["name"], "shell");
    }

    #[test]
    fn tool_results_to_json_roundtrip() {
        let content = vec![ContentBlock::ToolResult {
            tool_use_id: "tu_1".to_string(),
            content: "ok".to_string(),
            is_error: false,
        }];
        let json = tool_results_to_json(&content).unwrap();
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0]["tool_use_id"], "tu_1");
        assert_eq!(parsed[0]["content"], "ok");
    }

    #[test]
    fn tool_calls_to_json_returns_none_for_no_tool_content() {
        let content = vec![ContentBlock::Text {
            text: "Hello".to_string(),
        }];
        assert!(tool_calls_to_json(&content).is_none());
        assert!(tool_results_to_json(&content).is_none());
    }
}
