use std::sync::Arc;

use crate::db::GhostDb;
use async_trait::async_trait;
use serde_json::{Value, json};

use crate::agents::definition::TaskDefinition;
use crate::agents::{
    ContextPressureConfig, ProgressGateConfig, ProgressRule, RecencyConfig, TemporalConfig,
    ToolCountRule,
};
use crate::chat::compaction;
use crate::config::{self, Config};
use crate::db;
use crate::prompt::{PromptContext, PromptRenderer};
use crate::providers::{
    ChatMessage, ContentBlock, Provider, ReasoningEffort, Role, StopReason, provider_for_alias,
    resolve_reasoning_effort,
};
use crate::tools::{ToolContext, ToolManager, format_todo_injection};

use super::convert::{
    convert_stored_message_to_provider_message, parse_session_thing, render_tool_error,
    tool_results_to_values,
};
use super::tool_loop::{ToolLoopHandler, run_tool_loop};
use super::types::{
    ChatError, ChatResult, ChatStopReason, DEFAULT_MAX_TOOL_ITERATIONS, EventSender, RunMetadata,
    ToolLoopEvent,
};

pub struct SessionChat {
    db: GhostDb,
    provider: Arc<dyn Provider>,
    tool_manager: ToolManager,
    config: Config,
    prompt_renderer: PromptRenderer,
    max_tool_iterations: usize,
    task_runner: Option<Arc<crate::agents::TaskRunner>>,
}

impl std::fmt::Debug for SessionChat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionChat")
            .field("provider", &self.provider.name())
            .field("max_tool_iterations", &self.max_tool_iterations)
            .finish()
    }
}

impl SessionChat {
    #[tracing::instrument(name = "create session_chat", skip_all)]
    pub fn from_config(db: GhostDb, config: Config) -> Result<Self, ChatError> {
        let provider = provider_for_alias(&config, None)?;

        Ok(Self::new(db, provider, ToolManager::for_chat(), config))
    }

    #[must_use]
    pub fn new(
        db: GhostDb,
        provider: Arc<dyn Provider>,
        tool_manager: ToolManager,
        config: Config,
    ) -> Self {
        let prompt_renderer = PromptRenderer::new(config.clone());
        Self {
            db,
            provider,
            tool_manager,
            config,
            prompt_renderer,
            max_tool_iterations: DEFAULT_MAX_TOOL_ITERATIONS,
            task_runner: None,
        }
    }

    #[must_use]
    pub fn with_max_tool_iterations(mut self, max_tool_iterations: usize) -> Self {
        self.max_tool_iterations = max_tool_iterations;
        self
    }

    #[must_use]
    pub fn with_task_runner(mut self, runner: Arc<crate::agents::TaskRunner>) -> Self {
        self.task_runner = Some(runner);
        self
    }

    #[tracing::instrument(name = "orchestrate response", skip_all, fields(session_id = session_id))]
    pub async fn chat(
        &self,
        session_id: &str,
        user_message: &str,
        event_tx: Option<&EventSender>,
    ) -> Result<(ChatResult, RunMetadata), ChatError> {
        let session_thing = parse_session_thing(session_id)?;
        db::sessions::get_session(&self.db, &session_thing).await?;
        db::sessions::update_activity(&self.db, &session_thing).await?;
        db::sessions::create_message(&self.db, &session_thing, "user", user_message).await?;

        let (mut history, stored_ids) = self.load_provider_history(&session_thing).await?;
        self.compact_if_needed(&session_thing, &mut history, &stored_ids)
            .await;

        // TODO: Make that into a unified nudge system, like agents
        if let Some(todo_context) = self.todo_injection_message(&session_thing).await? {
            history.push(ChatMessage {
                role: Role::System,
                content: vec![ContentBlock::Text { text: todo_context }],
            });
        }

        let model = self.default_model_name()?;
        let effort = resolve_reasoning_effort(None, None, self.model_reasoning_effort());
        let mut handler = ChatHandler {
            session_chat: self,
            session_thing: &session_thing,
            event_tx,
            pending_todo_update: false,
        };

        run_tool_loop(
            self,
            session_id,
            &model,
            self.max_tool_iterations,
            effort,
            &mut handler,
            &mut history,
            event_tx,
        )
        .await
    }

    /// Run an agent tool loop with a custom system prompt.
    ///
    /// Messages are persisted to the agent's own session. Returns the final
    /// assistant message.
    pub async fn chat_agent(
        &self,
        session_id: &str,
        prompt: &str,
        system_prompt: String,
        definition: &TaskDefinition,
        event_tx: Option<&EventSender>,
    ) -> Result<(ChatResult, RunMetadata), ChatError> {
        let session_thing = parse_session_thing(session_id)?;
        db::sessions::create_message(&self.db, &session_thing, "user", prompt).await?;

        let model = self.default_model_name()?;
        let effort = resolve_reasoning_effort(
            None,
            definition.reasoning_effort,
            self.model_reasoning_effort(),
        );
        let mut history = vec![ChatMessage {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: prompt.to_string(),
            }],
        }];

        let context_window = self.model_context_window();

        let mut handler = TaskHandler {
            session_chat: self,
            session_thing: &session_thing,
            system_prompt,
            progress_rules: definition.progress_rules.clone(),
            progress_gate: definition.progress_gate.clone(),
            temporal: definition.temporal.clone(),
            recency: definition.recency.clone(),
            context_pressure: definition.context_pressure.clone(),
            context_window,
            last_input_tokens: 0,
            started_at: std::time::Instant::now(),
            temporal_nudge_count: 0,
            iteration_count: 0,
            max_iterations: definition.max_iterations,
            event_tx,
            pending_todo_update: false,
        };

        run_tool_loop(
            self,
            session_id,
            &model,
            definition.max_iterations,
            effort,
            &mut handler,
            &mut history,
            event_tx,
        )
        .await
    }

    /// Continue an existing agent session with a new user message.
    ///
    /// Loads the full history from the agent's DB session (all previous
    /// research + tool calls), appends the new user message, and runs the
    /// tool loop again. This lets agents refine their work without
    /// re-doing prior research.
    #[tracing::instrument(name = "continue agent", skip_all, fields(
        gen_ai.agent.name = %definition.name,
        session_id = session_id,
    ))]
    pub async fn continue_task(
        &self,
        session_id: &str,
        prompt: &str,
        system_prompt: String,
        definition: &TaskDefinition,
        event_tx: Option<&EventSender>,
    ) -> Result<(ChatResult, RunMetadata), ChatError> {
        let session_thing = parse_session_thing(session_id)?;

        // Store new user message in the existing agent session
        db::sessions::create_message(&self.db, &session_thing, "user", prompt).await?;

        let model = self.default_model_name()?;
        let effort = resolve_reasoning_effort(
            None,
            definition.reasoning_effort,
            self.model_reasoning_effort(),
        );
        // Load FULL history (all previous research + new user message)
        let (mut history, _stored_ids) = self.load_provider_history(&session_thing).await?;

        let context_window = self.model_context_window();

        let mut handler = TaskHandler {
            session_chat: self,
            session_thing: &session_thing,
            system_prompt,
            progress_rules: definition.progress_rules.clone(),
            progress_gate: definition.progress_gate.clone(),
            temporal: definition.temporal.clone(),
            recency: definition.recency.clone(),
            context_pressure: definition.context_pressure.clone(),
            context_window,
            last_input_tokens: 0,
            started_at: std::time::Instant::now(),
            temporal_nudge_count: 0,
            iteration_count: 0,
            max_iterations: definition.max_iterations,
            event_tx,
            pending_todo_update: false,
        };

        run_tool_loop(
            self,
            session_id,
            &model,
            definition.max_iterations,
            effort,
            &mut handler,
            &mut history,
            event_tx,
        )
        .await
    }

    #[tracing::instrument(name = "reboot session", skip_all, fields(old_session_id = session_id))]
    pub async fn reboot_session(&self, session_id: &str) -> Result<String, ChatError> {
        let old_session = parse_session_thing(session_id)?;
        db::sessions::mark_rebooted(&self.db, &old_session).await?;
        let new_session = db::sessions::create_session(&self.db).await?;
        db::interface_sessions::replace_session_everywhere(&self.db, &old_session, &new_session)
            .await?;
        Ok(new_session)
    }

    /// Load provider history, returning `(messages, stored_message_ids)`.
    ///
    /// `stored_message_ids` is parallel to the returned messages (one DB
    /// message ID per provider message). The summary pseudo-message (if any)
    /// gets an empty string as its ID.
    #[tracing::instrument(skip_all, level = "debug", fields(session_id = ?session_id))]
    pub(super) async fn load_provider_history(
        &self,
        session_id: &str,
    ) -> Result<(Vec<ChatMessage>, Vec<String>), ChatError> {
        let session = db::sessions::get_session(&self.db, session_id).await?;
        let all_messages = db::sessions::list_messages_by_session(&self.db, session_id).await?;

        let mut messages = Vec::new();
        let mut ids = Vec::new();

        if let Some(summary) = session.compaction_summary
            && !summary.trim().is_empty()
        {
            messages.push(ChatMessage {
                role: Role::System,
                content: vec![ContentBlock::Text { text: summary }],
            });
            ids.push(String::new());
        }

        let cursor = session.compaction_cursor_id;
        let mut include = cursor.is_none();
        for msg in all_messages {
            if !include {
                include = Some(msg.id.clone()) == cursor;
                continue;
            }
            let msg_id = msg.id.clone();
            messages.push(convert_stored_message_to_provider_message(msg));
            ids.push(msg_id);
        }

        Ok((messages, ids))
    }

    #[tracing::instrument(skip_all, level = "debug", fields(session_id = ?session_id))]
    pub(super) async fn todo_injection_message(
        &self,
        session_id: &str,
    ) -> Result<Option<String>, ChatError> {
        let todo = db::sessions::get_session_todo_list(&self.db, session_id).await?;
        let Some(items) = todo else {
            return Ok(None);
        };
        if items.is_empty() {
            return Ok(None);
        }
        Ok(Some(format_todo_injection(&items)))
    }

    #[tracing::instrument(name = "run tools", skip_all)]
    pub(super) async fn execute_tool_calls(
        &self,
        session_id: &str,
        tool_calls: &[Value],
    ) -> Vec<ContentBlock> {
        let futures: Vec<_> = tool_calls
            .iter()
            .filter_map(|call| {
                let id = call.get("id").and_then(Value::as_str)?;
                let name = call.get("name").and_then(Value::as_str)?;
                let input = call.get("input").cloned().unwrap_or_else(|| json!({}));
                Some(self.execute_single_tool(session_id, name, id, input))
            })
            .collect();

        futures::future::join_all(futures).await
    }

    async fn execute_single_tool(
        &self,
        session_id: &str,
        name: &str,
        tool_use_id: &str,
        input: Value,
    ) -> ContentBlock {
        let tool_ctx = ToolContext {
            workspace: self.config.workspace.clone(),
            cwd: self.config.workspace.clone(),
            db: self.db.clone(),
            config: self.config.clone(),
            session_id: session_id.to_string(),
            task_runner: self.task_runner.clone(),
        };

        match self.tool_manager.execute(name, input, &tool_ctx).await {
            Ok(content) => ContentBlock::ToolResult {
                tool_use_id: tool_use_id.to_string(),
                content,
                is_error: false,
            },
            Err(error) => ContentBlock::ToolResult {
                tool_use_id: tool_use_id.to_string(),
                content: render_tool_error(error),
                is_error: true,
            },
        }
    }

    pub(super) fn default_model_name(&self) -> Result<String, ChatError> {
        let alias = &self.config.models.default;
        let model = self.config.models.aliases.get(alias).ok_or_else(|| {
            ChatError::Config(config::ConfigError::UnknownDefaultModelAlias {
                alias: alias.clone(),
            })
        })?;
        Ok(model.model.clone())
    }

    pub(super) fn db(&self) -> &GhostDb {
        &self.db
    }

    pub(super) fn provider(&self) -> &Arc<dyn Provider> {
        &self.provider
    }

    pub(super) fn config(&self) -> &Config {
        &self.config
    }

    /// Context window size (in tokens) from the default model alias.
    fn model_context_window(&self) -> usize {
        self.config
            .models
            .aliases
            .get(&self.config.models.default)
            .map(|m| m.context_window as usize)
            .unwrap_or(200_000)
    }

    /// Reasoning effort configured on the default model alias, if any.
    fn model_reasoning_effort(&self) -> Option<ReasoningEffort> {
        self.config
            .models
            .aliases
            .get(&self.config.models.default)
            .and_then(|m| m.reasoning_effort)
    }

    pub(super) fn tool_manager(&self) -> &ToolManager {
        &self.tool_manager
    }

    pub(super) fn prompt_renderer(&self) -> &PromptRenderer {
        &self.prompt_renderer
    }
}

// ---------------------------------------------------------------------------
// ToolLoopHandler implementations
// ---------------------------------------------------------------------------

struct ChatHandler<'a> {
    session_chat: &'a SessionChat,
    session_thing: &'a str,
    event_tx: Option<&'a EventSender>,
    pending_todo_update: bool,
}

#[async_trait]
impl ToolLoopHandler for ChatHandler<'_> {
    fn system_prompt(&self) -> Result<String, ChatError> {
        let model = self.session_chat.default_model_name()?;
        self.session_chat
            .prompt_renderer()
            .render_system_prompt(&PromptContext {
                model,
                provider: self.session_chat.provider().name().to_string(),
            })
            .map_err(ChatError::from)
    }

    async fn on_assistant_tool_use(
        &mut self,
        text: &str,
        tool_uses: &[Value],
        raw_output: Option<Vec<Value>>,
    ) -> Result<(), ChatError> {
        // Detect todo tool calls for live TODO updates
        self.pending_todo_update = tool_uses
            .iter()
            .any(|t| t.get("name").and_then(Value::as_str) == Some("todo"));

        db::sessions::create_message_with_metadata(
            self.session_chat.db(),
            self.session_thing,
            "assistant",
            text,
            Some(tool_uses.to_vec()),
            None,
            raw_output,
        )
        .await?;
        Ok(())
    }

    async fn on_tool_results(&mut self, results: &[ContentBlock]) -> Result<(), ChatError> {
        db::sessions::create_message_with_metadata(
            self.session_chat.db(),
            self.session_thing,
            "user",
            "",
            None,
            Some(tool_results_to_values(results)),
            None,
        )
        .await?;
        Ok(())
    }

    async fn on_end_turn(
        &mut self,
        message: String,
        stop_reason: StopReason,
        tool_uses: &[Value],
        raw_output: Option<Vec<Value>>,
    ) -> Result<ChatResult, ChatError> {
        db::sessions::create_message_with_metadata(
            self.session_chat.db(),
            self.session_thing,
            "assistant",
            &message,
            Some(tool_uses.to_vec()),
            None,
            raw_output,
        )
        .await?;

        Ok(ChatResult {
            message,
            stop_reason: if stop_reason == StopReason::MaxTokens {
                ChatStopReason::MaxTokens
            } else {
                ChatStopReason::EndTurn
            },
        })
    }

    async fn post_tool_iteration(
        &mut self,
        history: &mut Vec<ChatMessage>,
        _last_input_tokens: u32,
    ) -> Result<(), ChatError> {
        self.session_chat.apply_masking_if_needed(history);

        let todo_items =
            db::sessions::get_session_todo_list(self.session_chat.db(), self.session_thing).await?;

        if let Some(ref items) = todo_items
            && !items.is_empty()
        {
            history.push(ChatMessage {
                role: Role::System,
                content: vec![ContentBlock::Text {
                    text: format_todo_injection(items),
                }],
            });

            // Emit TODO event if a todo tool was called this iteration
            if self.pending_todo_update {
                if let Some(tx) = self.event_tx {
                    let _ = tx.send(ToolLoopEvent::TodoUpdated {
                        items: items.clone(),
                    });
                }
                self.pending_todo_update = false;
            }
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// TaskHandler — persists to DB, uses agent's system prompt
// ---------------------------------------------------------------------------

struct TaskHandler<'a> {
    session_chat: &'a SessionChat,
    session_thing: &'a str,
    system_prompt: String,
    progress_rules: Vec<ProgressRule>,
    progress_gate: Option<ProgressGateConfig>,
    temporal: Option<TemporalConfig>,
    recency: Option<RecencyConfig>,
    context_pressure: Option<ContextPressureConfig>,
    context_window: usize,
    last_input_tokens: u32,
    started_at: std::time::Instant,
    temporal_nudge_count: usize,
    iteration_count: usize,
    max_iterations: usize,
    event_tx: Option<&'a EventSender>,
    pending_todo_update: bool,
}

#[async_trait]
impl ToolLoopHandler for TaskHandler<'_> {
    fn system_prompt(&self) -> Result<String, ChatError> {
        Ok(self.system_prompt.clone())
    }

    async fn on_assistant_tool_use(
        &mut self,
        text: &str,
        tool_uses: &[Value],
        raw_output: Option<Vec<Value>>,
    ) -> Result<(), ChatError> {
        self.pending_todo_update = tool_uses
            .iter()
            .any(|t| t.get("name").and_then(Value::as_str) == Some("todo"));

        db::sessions::create_message_with_metadata(
            self.session_chat.db(),
            self.session_thing,
            "assistant",
            text,
            Some(tool_uses.to_vec()),
            None,
            raw_output,
        )
        .await?;
        Ok(())
    }

    async fn on_tool_results(&mut self, results: &[ContentBlock]) -> Result<(), ChatError> {
        db::sessions::create_message_with_metadata(
            self.session_chat.db(),
            self.session_thing,
            "user",
            "",
            None,
            Some(tool_results_to_values(results)),
            None,
        )
        .await?;
        Ok(())
    }

    async fn on_end_turn(
        &mut self,
        message: String,
        stop_reason: StopReason,
        tool_uses: &[Value],
        raw_output: Option<Vec<Value>>,
    ) -> Result<ChatResult, ChatError> {
        db::sessions::create_message_with_metadata(
            self.session_chat.db(),
            self.session_thing,
            "assistant",
            &message,
            Some(tool_uses.to_vec()),
            None,
            raw_output,
        )
        .await?;

        Ok(ChatResult {
            message,
            stop_reason: if stop_reason == StopReason::MaxTokens {
                ChatStopReason::MaxTokens
            } else {
                ChatStopReason::EndTurn
            },
        })
    }

    async fn post_tool_iteration(
        &mut self,
        history: &mut Vec<ChatMessage>,
        last_input_tokens: u32,
    ) -> Result<(), ChatError> {
        self.last_input_tokens = last_input_tokens;
        self.session_chat.apply_masking_if_needed(history);

        // Inject TODO as a separate plain-text system message.
        // Remove any previous TODO injection first to avoid stacking.
        let todo_items =
            db::sessions::get_session_todo_list(self.session_chat.db(), self.session_thing).await?;

        if let Some(ref items) = todo_items
            && !items.is_empty()
        {
            history.retain(|m| {
                !(m.role == Role::System
                    && m.content.iter().any(|b| {
                        matches!(b, ContentBlock::Text { text } if text.starts_with("Current TODO"))
                    }))
            });
            history.push(ChatMessage {
                role: Role::System,
                content: vec![ContentBlock::Text {
                    text: format_todo_injection(items),
                }],
            });

            if self.pending_todo_update {
                if let Some(tx) = self.event_tx {
                    let _ = tx.send(ToolLoopEvent::TodoUpdated {
                        items: items.clone(),
                    });
                }
                self.pending_todo_update = false;
            }
        }

        // Inject progress nudge (periodic rules) as a separate system message.
        if let Some(nudge) = build_progress_nudge(&self.progress_rules, history) {
            history.push(ChatMessage {
                role: Role::System,
                content: vec![ContentBlock::Text { text: nudge }],
            });
        }

        // Event-driven "tool not used recently" reminder (config-driven).
        if let Some(reminder) = build_recency_reminder(history, self.recency.as_ref()) {
            history.push(ChatMessage {
                role: Role::System,
                content: vec![ContentBlock::Text { text: reminder }],
            });
        }

        // Context-pressure nudge (config-driven, percentage-based).
        if let Some(reminder) = build_context_pressure_reminder(
            history,
            self.context_pressure.as_ref(),
            self.context_window,
            self.last_input_tokens,
        ) {
            history.push(ChatMessage {
                role: Role::System,
                content: vec![ContentBlock::Text { text: reminder }],
            });
        }

        // Iteration countdown nudge (config-driven). Fires every turn
        // within a band, picking the most urgent applicable rule.
        self.iteration_count += 1;
        let remaining = self.max_iterations.saturating_sub(self.iteration_count);
        if let Some(reminder) = build_iteration_countdown_nudge(&self.progress_rules, remaining) {
            history.push(ChatMessage {
                role: Role::System,
                content: vec![ContentBlock::Text { text: reminder }],
            });
        }

        // Temporal nudge (config-driven). Fires every iteration past
        // the threshold to keep pressuring the agent to wrap up.
        if let Some(reminder) = build_temporal_nudge(
            self.started_at,
            self.temporal.as_ref(),
            self.temporal_nudge_count,
        ) {
            self.temporal_nudge_count += 1;
            history.push(ChatMessage {
                role: Role::System,
                content: vec![ContentBlock::Text { text: reminder }],
            });
        }

        Ok(())
    }

    async fn check_progress_gate(
        &mut self,
        _history: &[ChatMessage],
    ) -> Result<Option<String>, ChatError> {
        let Some(ref gate) = self.progress_gate else {
            return Ok(None);
        };

        // If the temporal nudge has fired, let the model write — it's been
        // told to wrap up and we don't want to contradict that.
        if self.temporal_nudge_count > 0 {
            return Ok(None);
        }

        use crate::tools::TodoStatus;

        let todo_items =
            db::sessions::get_session_todo_list(self.session_chat.db(), self.session_thing).await?;

        // If no TODO exists, the agent skipped the planning step.
        if todo_items.is_none() {
            return Ok(Some(format!(
                "<system-reminder>{}</system-reminder>",
                gate.no_todo
            )));
        }

        // Block ending while TODO items remain incomplete.
        let incomplete = todo_items
            .unwrap()
            .iter()
            .filter(|i| matches!(i.status, TodoStatus::Pending | TodoStatus::InProgress))
            .count();

        if incomplete > 0 {
            let msg = gate
                .incomplete
                .replace("{incomplete}", &incomplete.to_string());
            return Ok(Some(format!("<system-reminder>{msg}</system-reminder>")));
        }

        Ok(None)
    }
}

/// Check if a key tool hasn't been used recently and inject a reminder.
///
/// Fires when the agent has used tools in the last N assistant turns but
/// none of them were the configured tool. Returns `None` when no recency
/// config is present.
fn build_recency_reminder(
    history: &[ChatMessage],
    config: Option<&RecencyConfig>,
) -> Option<String> {
    let config = config?;

    let recent_assistant: Vec<&ChatMessage> = history
        .iter()
        .rev()
        .filter(|m| m.role == Role::Assistant)
        .take(config.window)
        .collect();

    if recent_assistant.len() < config.window {
        return None;
    }

    // Check if ANY of the last N assistant turns had the tracked tool
    let has_recent_use = recent_assistant.iter().any(|msg| {
        msg.content.iter().any(
            |block| matches!(block, ContentBlock::ToolUse { name, .. } if name == &config.tool),
        )
    });

    if has_recent_use {
        return None;
    }

    // Check that at least one recent turn had tool calls (not just text)
    let has_tool_calls = recent_assistant.iter().any(|msg| {
        msg.content
            .iter()
            .any(|block| matches!(block, ContentBlock::ToolUse { .. }))
    });

    if !has_tool_calls {
        return None;
    }

    Some(format!(
        "<system-reminder>{}</system-reminder>",
        config.message
    ))
}

/// Nudge when estimated token usage approaches the context window.
///
/// Uses the actual `input_tokens` from the last provider response as a
/// reliable base, then adds estimated tokens for content appended since
/// (tool results, nudge messages). Fires once when the ratio exceeds
/// `threshold_pct` of `context_window`.
fn build_context_pressure_reminder(
    history: &[ChatMessage],
    config: Option<&ContextPressureConfig>,
    context_window: usize,
    last_input_tokens: u32,
) -> Option<String> {
    let config = config?;

    if context_window == 0 || last_input_tokens == 0 {
        return None;
    }

    // Estimate tokens for content added after the last API response
    // (tool results and any nudge messages appended in post_tool_iteration).
    // The last two messages are typically: assistant tool_use + user tool_results.
    let new_content_tokens: usize = history
        .iter()
        .rev()
        .take(2)
        .map(compaction::estimate_message_tokens)
        .sum();

    let estimated_next_input = last_input_tokens as usize + new_content_tokens;
    let ratio = estimated_next_input as f64 / context_window as f64;

    if ratio < config.threshold_pct {
        return None;
    }

    // Only fire once — check if we already injected this reminder
    let already_nudged = history.iter().any(|m| {
        m.role == Role::System
            && m.content.iter().any(|b| {
                matches!(
                    b,
                    ContentBlock::Text { text } if text.contains(&config.message)
                )
            })
    });

    if already_nudged {
        return None;
    }

    let pct = (ratio * 100.0).round() as u32;
    logfire::warn!(
        "context pressure nudge fired",
        estimated_tokens = estimated_next_input as u64,
        context_window = context_window as u64,
        usage_pct = pct as u64,
    );

    Some(format!(
        "<system-reminder>{}</system-reminder>",
        config.message
    ))
}

/// Nudge when wall-clock time exceeds a threshold.
/// Returns `None` when no config is present or before the threshold.
/// Fires on **every** call past the threshold (not just once) so the
/// agent keeps getting pressure to wrap up.
///
/// `fire_count` is 0-indexed: 0 for the first fire, 1 for the second, etc.
/// The config's `message` list is indexed accordingly; the last element
/// repeats for all subsequent fires.
///
/// `{minutes}` is interpolated into the selected message.
fn build_temporal_nudge(
    started_at: std::time::Instant,
    config: Option<&TemporalConfig>,
    fire_count: usize,
) -> Option<String> {
    let config = config?;

    let threshold = std::time::Duration::from_secs(config.after_seconds);
    if started_at.elapsed() < threshold {
        return None;
    }

    let idx = fire_count.min(config.message.len() - 1);
    let mins = started_at.elapsed().as_secs() / 60;
    let msg = config.message[idx].replace("{minutes}", &mins.to_string());
    Some(format!("<system-reminder>{msg}</system-reminder>"))
}

/// Build a progress nudge from declared tool-count rules.
///
/// Returns `None` if there are no tool-count rules or no tracked tools
/// have been called yet. Otherwise returns an XML `<progress>` block
/// wrapped in `<system-reminder>`.
fn build_progress_nudge(rules: &[ProgressRule], history: &[ChatMessage]) -> Option<String> {
    let tool_rules: Vec<&ToolCountRule> = rules
        .iter()
        .filter_map(|r| match r {
            ProgressRule::ToolCount(tc) => Some(tc),
            _ => None,
        })
        .collect();

    if tool_rules.is_empty() {
        return None;
    }

    // Count calls to each tracked tool
    let mut counts = std::collections::HashMap::<&str, u32>::new();
    for msg in history {
        if msg.role != Role::Assistant {
            continue;
        }
        for block in &msg.content {
            if let ContentBlock::ToolUse { name, .. } = block
                && tool_rules.iter().any(|r| r.tool == name.as_str())
            {
                *counts.entry(name.as_str()).or_default() += 1;
            }
        }
    }

    // Don't nudge before any tracked tool has been called
    if counts.is_empty() {
        return None;
    }

    let mut tool_elements = Vec::new();
    for rule in &tool_rules {
        let count = counts.get(rule.tool.as_str()).copied().unwrap_or(0);

        let nudge_text = match (&rule.min, &rule.nudge) {
            (Some(min), Some(nudge)) if count < *min => {
                let interpolated = nudge
                    .replace("{tool}", &rule.tool)
                    .replace("{count}", &count.to_string())
                    .replace("{min}", &min.to_string());
                Some(interpolated)
            }
            (None, Some(nudge)) => {
                let interpolated = nudge
                    .replace("{tool}", &rule.tool)
                    .replace("{count}", &count.to_string())
                    .replace("{min}", "");
                Some(interpolated)
            }
            _ => None,
        };

        let min_attr = rule
            .min
            .map(|m| format!(" min=\"{m}\""))
            .unwrap_or_default();

        if let Some(text) = nudge_text {
            tool_elements.push(format!(
                "<tool name=\"{}\" count=\"{count}\"{min_attr}>\n{text}\n</tool>",
                rule.tool
            ));
        } else {
            tool_elements.push(format!(
                "<tool name=\"{}\" count=\"{count}\"{min_attr} />",
                rule.tool
            ));
        }
    }

    Some(format!(
        "<system-reminder>\n<progress>\n{}\n</progress>\n</system-reminder>",
        tool_elements.join("\n")
    ))
}

/// Build an iteration countdown nudge from declared rules.
///
/// Among all countdown rules where `remaining <= rule.remaining_iterations`,
/// fires only the one with the **lowest** `remaining_iterations` value
/// (the most urgent). Interpolates `{remaining}` in the message.
fn build_iteration_countdown_nudge(rules: &[ProgressRule], remaining: usize) -> Option<String> {
    let best = rules
        .iter()
        .filter_map(|r| match r {
            ProgressRule::IterationCountdown(ic) => Some(ic),
            _ => None,
        })
        .filter(|ic| remaining <= ic.remaining_iterations)
        .min_by_key(|ic| ic.remaining_iterations)?;

    let msg = best.message.replace("{remaining}", &remaining.to_string());
    Some(format!("<system-reminder>{msg}</system-reminder>"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(tool: &str, min: u32) -> ProgressRule {
        ProgressRule::ToolCount(ToolCountRule {
            tool: tool.to_string(),
            min: Some(min),
            nudge: None,
        })
    }

    fn rule_with_nudge(tool: &str, min: Option<u32>, nudge: &str) -> ProgressRule {
        ProgressRule::ToolCount(ToolCountRule {
            tool: tool.to_string(),
            min,
            nudge: Some(nudge.to_string()),
        })
    }

    fn assistant_tool_use(name: &str) -> ChatMessage {
        ChatMessage {
            role: Role::Assistant,
            content: vec![ContentBlock::ToolUse {
                id: "test".to_string(),
                name: name.to_string(),
                input: json!({}),
            }],
        }
    }

    #[test]
    fn no_rules_returns_none() {
        let history = vec![assistant_tool_use("web_fetch")];
        assert!(build_progress_nudge(&[], &history).is_none());
    }

    #[test]
    fn no_tool_calls_returns_none() {
        let rules = vec![rule("web_fetch", 5)];
        let history = vec![ChatMessage {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: "hello".to_string(),
            }],
        }];
        assert!(build_progress_nudge(&rules, &history).is_none());
    }

    #[test]
    fn below_minimum_default_nudge_shows_xml() {
        let rules = vec![rule_with_nudge(
            "web_fetch",
            Some(5),
            "Need {min} {tool} (have {count}). Keep going.",
        )];
        let history = vec![
            assistant_tool_use("web_fetch"),
            assistant_tool_use("web_fetch"),
        ];
        let nudge = build_progress_nudge(&rules, &history).unwrap();
        assert!(nudge.contains("<system-reminder>"));
        assert!(nudge.contains("<progress>"));
        assert!(nudge.contains("count=\"2\""));
        assert!(nudge.contains("min=\"5\""));
        assert!(nudge.contains("Need 5 web_fetch (have 2). Keep going."));
    }

    #[test]
    fn at_minimum_no_nudge_self_closing() {
        let rules = vec![rule("web_fetch", 2)];
        let history = vec![
            assistant_tool_use("web_fetch"),
            assistant_tool_use("web_fetch"),
        ];
        let nudge = build_progress_nudge(&rules, &history).unwrap();
        assert!(nudge.contains("count=\"2\""));
        assert!(nudge.contains("min=\"2\""));
        assert!(nudge.contains("/>"));
        // No nudge text when no nudge is defined and min is met
        assert!(!nudge.contains("Keep going"));
    }

    #[test]
    fn above_minimum_no_nudge_self_closing() {
        let rules = vec![rule("web_fetch", 2)];
        let history = vec![
            assistant_tool_use("web_fetch"),
            assistant_tool_use("web_fetch"),
            assistant_tool_use("web_fetch"),
        ];
        let nudge = build_progress_nudge(&rules, &history).unwrap();
        assert!(nudge.contains("count=\"3\""));
        assert!(nudge.contains("/>"));
    }

    #[test]
    fn at_minimum_with_nudge_stops_nudging() {
        let rules = vec![rule_with_nudge(
            "web_fetch",
            Some(2),
            "Below minimum — keep going.",
        )];
        let history = vec![
            assistant_tool_use("web_fetch"),
            assistant_tool_use("web_fetch"),
        ];
        let nudge = build_progress_nudge(&rules, &history).unwrap();
        // min is met → nudge should NOT fire, self-closing
        assert!(nudge.contains("/>"));
        assert!(!nudge.contains("Below minimum"));
    }

    #[test]
    fn no_min_with_nudge_always_fires() {
        let rules = vec![rule_with_nudge(
            "note_write",
            None,
            "You have created {count} notes so far.",
        )];
        let history = vec![
            assistant_tool_use("note_write"),
            assistant_tool_use("note_write"),
        ];
        let nudge = build_progress_nudge(&rules, &history).unwrap();
        assert!(nudge.contains("count=\"2\""));
        assert!(!nudge.contains("min="));
        assert!(nudge.contains("You have created 2 notes so far."));
    }

    #[test]
    fn no_min_no_nudge_self_closing() {
        let rules = vec![ProgressRule::ToolCount(ToolCountRule {
            tool: "note_write".to_string(),
            min: None,
            nudge: None,
        })];
        let history = vec![assistant_tool_use("note_write")];
        let nudge = build_progress_nudge(&rules, &history).unwrap();
        assert!(nudge.contains("count=\"1\""));
        assert!(nudge.contains("/>"));
    }

    #[test]
    fn custom_nudge_with_interpolation() {
        let rules = vec![rule_with_nudge(
            "web_fetch",
            Some(5),
            "Need {min} {tool} (have {count}).",
        )];
        let history = vec![
            assistant_tool_use("web_fetch"),
            assistant_tool_use("web_fetch"),
        ];
        let nudge = build_progress_nudge(&rules, &history).unwrap();
        assert!(nudge.contains("Need 5 web_fetch (have 2)."));

        // Now at minimum — nudge should stop
        let mut history_met = history.clone();
        for _ in 0..3 {
            history_met.push(assistant_tool_use("web_fetch"));
        }
        let nudge_met = build_progress_nudge(&rules, &history_met).unwrap();
        assert!(nudge_met.contains("count=\"5\""));
        assert!(nudge_met.contains("/>"));
        assert!(!nudge_met.contains("Need 5"));
    }

    #[test]
    fn ignores_untracked_tools() {
        let rules = vec![rule("web_fetch", 3)];
        let history = vec![
            assistant_tool_use("web_search"),
            assistant_tool_use("web_search"),
        ];
        // web_search isn't tracked, so no tracked calls → None
        assert!(build_progress_nudge(&rules, &history).is_none());
    }

    // --- Config-driven nudge tests ---

    #[test]
    fn recency_none_config_returns_none() {
        let history = vec![
            assistant_tool_use("web_search"),
            assistant_tool_use("web_search"),
            assistant_tool_use("web_search"),
        ];
        assert!(build_recency_reminder(&history, None).is_none());
    }

    #[test]
    fn recency_fires_when_tool_absent() {
        let config = RecencyConfig {
            tool: "web_fetch".to_string(),
            window: 3,
            message: "Fetch something.".to_string(),
        };
        let history = vec![
            assistant_tool_use("web_search"),
            assistant_tool_use("web_search"),
            assistant_tool_use("web_search"),
        ];
        let nudge = build_recency_reminder(&history, Some(&config)).unwrap();
        assert!(nudge.contains("Fetch something."));
        assert!(nudge.contains("<system-reminder>"));
    }

    #[test]
    fn recency_silent_when_tool_present() {
        let config = RecencyConfig {
            tool: "web_fetch".to_string(),
            window: 3,
            message: "Fetch something.".to_string(),
        };
        let history = vec![
            assistant_tool_use("web_search"),
            assistant_tool_use("web_fetch"),
            assistant_tool_use("web_search"),
        ];
        assert!(build_recency_reminder(&history, Some(&config)).is_none());
    }

    #[test]
    fn temporal_none_config_returns_none() {
        let started = std::time::Instant::now() - std::time::Duration::from_secs(600);
        assert!(build_temporal_nudge(started, None, 0).is_none());
    }

    #[test]
    fn temporal_fires_after_threshold() {
        let config = TemporalConfig {
            after_seconds: 60,
            message: vec!["Been working {minutes} min.".to_string()],
        };
        let started = std::time::Instant::now() - std::time::Duration::from_secs(120);
        let nudge = build_temporal_nudge(started, Some(&config), 0).unwrap();
        assert!(nudge.contains("Been working 2 min."));
    }

    #[test]
    fn temporal_fires_repeatedly() {
        let config = TemporalConfig {
            after_seconds: 60,
            message: vec!["Wrap up.".to_string()],
        };
        let started = std::time::Instant::now() - std::time::Duration::from_secs(120);
        // Should fire every time past the threshold, not just once
        assert!(build_temporal_nudge(started, Some(&config), 0).is_some());
        assert!(build_temporal_nudge(started, Some(&config), 1).is_some());
    }

    #[test]
    fn temporal_skips_before_threshold() {
        let config = TemporalConfig {
            after_seconds: 300,
            message: vec!["Wrap up.".to_string()],
        };
        let started = std::time::Instant::now();
        assert!(build_temporal_nudge(started, Some(&config), 0).is_none());
    }

    #[test]
    fn temporal_escalates_messages() {
        let config = TemporalConfig {
            after_seconds: 60,
            message: vec![
                "Gentle: wrap up.".to_string(),
                "Firm: stop now.".to_string(),
                "Final: report immediately.".to_string(),
            ],
        };
        let started = std::time::Instant::now() - std::time::Duration::from_secs(120);

        let n0 = build_temporal_nudge(started, Some(&config), 0).unwrap();
        assert!(n0.contains("Gentle"), "first fire should use index 0");

        let n1 = build_temporal_nudge(started, Some(&config), 1).unwrap();
        assert!(n1.contains("Firm"), "second fire should use index 1");

        let n2 = build_temporal_nudge(started, Some(&config), 2).unwrap();
        assert!(n2.contains("Final"), "third fire should use index 2");

        // Beyond the list — last element repeats
        let n5 = build_temporal_nudge(started, Some(&config), 5).unwrap();
        assert!(n5.contains("Final"), "past end should repeat last");
    }

    #[test]
    fn context_pressure_none_config_returns_none() {
        let history = vec![ChatMessage {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: "x".repeat(300_000),
            }],
        }];
        assert!(build_context_pressure_reminder(&history, None, 200_000, 150_000).is_none());
    }

    #[test]
    fn context_pressure_fires_above_threshold() {
        let config = ContextPressureConfig {
            threshold_pct: 0.70,
            message: "Context large.".to_string(),
        };
        // Simulate: last response used 80_000 input tokens out of 100_000 window (80%)
        let history = vec![ChatMessage {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: "short".to_string(),
            }],
        }];
        let nudge =
            build_context_pressure_reminder(&history, Some(&config), 100_000, 80_000).unwrap();
        assert!(nudge.contains("Context large."));
    }

    #[test]
    fn context_pressure_does_not_fire_below_threshold() {
        let config = ContextPressureConfig {
            threshold_pct: 0.70,
            message: "Context large.".to_string(),
        };
        // Simulate: last response used 50_000 input tokens out of 100_000 window (50%)
        let history = vec![ChatMessage {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: "short".to_string(),
            }],
        }];
        assert!(
            build_context_pressure_reminder(&history, Some(&config), 100_000, 50_000).is_none()
        );
    }

    #[test]
    fn context_pressure_fires_once() {
        let config = ContextPressureConfig {
            threshold_pct: 0.70,
            message: "Context large.".to_string(),
        };
        let history = vec![
            ChatMessage {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: "short".to_string(),
                }],
            },
            ChatMessage {
                role: Role::System,
                content: vec![ContentBlock::Text {
                    text: "<system-reminder>Context large.</system-reminder>".to_string(),
                }],
            },
        ];
        // Even though tokens are high, already nudged — should not fire again
        assert!(
            build_context_pressure_reminder(&history, Some(&config), 100_000, 80_000).is_none()
        );
    }

    // --- Iteration countdown nudge tests ---

    use crate::agents::IterationCountdownRule;

    fn countdown_rules() -> Vec<ProgressRule> {
        vec![
            ProgressRule::IterationCountdown(IterationCountdownRule {
                remaining_iterations: 10,
                message: "{remaining} iterations left. Wrap up.".to_string(),
            }),
            ProgressRule::IterationCountdown(IterationCountdownRule {
                remaining_iterations: 5,
                message: "{remaining} left. Write report NOW.".to_string(),
            }),
        ]
    }

    #[test]
    fn countdown_no_rules_returns_none() {
        assert!(build_iteration_countdown_nudge(&[], 8).is_none());
    }

    #[test]
    fn countdown_above_all_thresholds_returns_none() {
        let rules = countdown_rules();
        assert!(build_iteration_countdown_nudge(&rules, 12).is_none());
    }

    #[test]
    fn countdown_fires_first_band() {
        let rules = countdown_rules();
        let nudge = build_iteration_countdown_nudge(&rules, 8).unwrap();
        assert!(nudge.contains("8 iterations left. Wrap up."));
        assert!(nudge.contains("<system-reminder>"));
    }

    #[test]
    fn countdown_fires_at_exact_threshold() {
        let rules = countdown_rules();
        let nudge = build_iteration_countdown_nudge(&rules, 10).unwrap();
        assert!(nudge.contains("10 iterations left. Wrap up."));
    }

    #[test]
    fn countdown_escalates_to_most_urgent() {
        let rules = countdown_rules();
        let nudge = build_iteration_countdown_nudge(&rules, 4).unwrap();
        // Should pick the 5-remaining rule (most urgent applicable)
        assert!(nudge.contains("4 left. Write report NOW."));
        assert!(!nudge.contains("iterations left. Wrap up."));
    }

    #[test]
    fn countdown_fires_at_zero() {
        let rules = countdown_rules();
        let nudge = build_iteration_countdown_nudge(&rules, 0).unwrap();
        assert!(nudge.contains("0 left. Write report NOW."));
    }

    #[test]
    fn countdown_ignores_tool_count_rules() {
        let rules = vec![rule("web_fetch", 5)];
        assert!(build_iteration_countdown_nudge(&rules, 3).is_none());
    }
}
