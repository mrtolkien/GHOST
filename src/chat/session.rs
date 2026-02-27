use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};
use surrealdb::Surreal;
use surrealdb::engine::local::Db;
use surrealdb::types::RecordId;

use crate::agents::definition::TaskDefinition;
use crate::agents::{
    ContextPressureConfig, ProgressGateConfig, ProgressRule, RecencyConfig, TemporalConfig,
};
use crate::config::{self, Config};
use crate::db;
use crate::prompt::{PromptContext, PromptRenderer};
use crate::providers::{ChatMessage, ContentBlock, Provider, Role, StopReason, provider_for_alias};
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
    db: Surreal<Db>,
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
    pub fn from_config(db: Surreal<Db>, config: Config) -> Result<Self, ChatError> {
        let provider = provider_for_alias(&config, None)?;

        Ok(Self::new(db, provider, ToolManager::for_chat(), config))
    }

    #[must_use]
    pub fn new(
        db: Surreal<Db>,
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
        let mut history = vec![ChatMessage {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: prompt.to_string(),
            }],
        }];

        let mut handler = TaskHandler {
            session_chat: self,
            session_thing: &session_thing,
            system_prompt,
            progress_rules: definition.progress_rules.clone(),
            progress_gate: definition.progress_gate.clone(),
            temporal: definition.temporal.clone(),
            recency: definition.recency.clone(),
            context_pressure: definition.context_pressure.clone(),
            started_at: std::time::Instant::now(),
            temporal_nudge_fired: false,
            event_tx,
            pending_todo_update: false,
        };

        run_tool_loop(
            self,
            session_id,
            &model,
            definition.max_iterations,
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
        // Load FULL history (all previous research + new user message)
        let (mut history, _stored_ids) = self.load_provider_history(&session_thing).await?;

        let mut handler = TaskHandler {
            session_chat: self,
            session_thing: &session_thing,
            system_prompt,
            progress_rules: definition.progress_rules.clone(),
            progress_gate: definition.progress_gate.clone(),
            temporal: definition.temporal.clone(),
            recency: definition.recency.clone(),
            context_pressure: definition.context_pressure.clone(),
            started_at: std::time::Instant::now(),
            temporal_nudge_fired: false,
            event_tx,
            pending_todo_update: false,
        };

        run_tool_loop(
            self,
            session_id,
            &model,
            definition.max_iterations,
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
        Ok(crate::db::fmt_id(&new_session))
    }

    /// Load provider history, returning `(messages, stored_message_ids)`.
    ///
    /// `stored_message_ids` is parallel to the returned messages (one DB
    /// message ID per provider message). The summary pseudo-message (if any)
    /// gets an empty string as its ID.
    #[tracing::instrument(skip_all, level = "debug", fields(session_id = ?session_id))]
    pub(super) async fn load_provider_history(
        &self,
        session_id: &RecordId,
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
                include = Some(crate::db::fmt_id(&msg.id)) == cursor;
                continue;
            }
            let msg_id = crate::db::fmt_id(&msg.id);
            messages.push(convert_stored_message_to_provider_message(msg));
            ids.push(msg_id);
        }

        Ok((messages, ids))
    }

    #[tracing::instrument(skip_all, level = "debug", fields(session_id = ?session_id))]
    pub(super) async fn todo_injection_message(
        &self,
        session_id: &RecordId,
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
        let mut results = Vec::new();
        for call in tool_calls {
            let Some(id) = call.get("id").and_then(Value::as_str) else {
                continue;
            };
            let Some(name) = call.get("name").and_then(Value::as_str) else {
                continue;
            };
            let input = call.get("input").cloned().unwrap_or_else(|| json!({}));
            let tool_result = self.execute_single_tool(session_id, name, id, input).await;
            results.push(tool_result);
        }
        results
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

    pub(super) fn db(&self) -> &Surreal<Db> {
        &self.db
    }

    pub(super) fn provider(&self) -> &Arc<dyn Provider> {
        &self.provider
    }

    pub(super) fn config(&self) -> &Config {
        &self.config
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
    session_thing: &'a RecordId,
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
    session_thing: &'a RecordId,
    system_prompt: String,
    progress_rules: Vec<ProgressRule>,
    progress_gate: Option<ProgressGateConfig>,
    temporal: Option<TemporalConfig>,
    recency: Option<RecencyConfig>,
    context_pressure: Option<ContextPressureConfig>,
    started_at: std::time::Instant,
    temporal_nudge_fired: bool,
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
    ) -> Result<(), ChatError> {
        self.session_chat.apply_masking_if_needed(history);

        // Inject TODO as a separate plain-text system message.
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

        // Context-pressure nudge (config-driven).
        if let Some(reminder) =
            build_context_pressure_reminder(history, self.context_pressure.as_ref())
        {
            history.push(ChatMessage {
                role: Role::System,
                content: vec![ContentBlock::Text { text: reminder }],
            });
        }

        // Temporal nudge (config-driven). Fires every iteration past
        // the threshold to keep pressuring the agent to wrap up.
        if let Some(reminder) = build_temporal_nudge(self.started_at, self.temporal.as_ref()) {
            self.temporal_nudge_fired = true;
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
        if self.temporal_nudge_fired {
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

/// Nudge when accumulated conversation content is getting large.
///
/// Estimates total content size from tool results and assistant text. Fires
/// once when the threshold is exceeded. Returns `None` when no config is
/// present.
fn build_context_pressure_reminder(
    history: &[ChatMessage],
    config: Option<&ContextPressureConfig>,
) -> Option<String> {
    let config = config?;

    let total_chars: usize = history
        .iter()
        .flat_map(|m| &m.content)
        .map(|block| match block {
            ContentBlock::Text { text } => text.len(),
            ContentBlock::ToolResult { content, .. } => content.len(),
            _ => 0,
        })
        .sum();

    if total_chars < config.threshold_chars {
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

    Some(format!(
        "<system-reminder>{}</system-reminder>",
        config.message
    ))
}

/// Nudge when wall-clock time exceeds a threshold. Fires once.
/// Returns `None` when no config is present or before the threshold.
/// Fires on **every** call past the threshold (not just once) so the
/// agent keeps getting pressure to wrap up.
/// `{minutes}` is interpolated into the message.
fn build_temporal_nudge(
    started_at: std::time::Instant,
    config: Option<&TemporalConfig>,
) -> Option<String> {
    let config = config?;

    let threshold = std::time::Duration::from_secs(config.after_seconds);
    if started_at.elapsed() < threshold {
        return None;
    }

    let mins = started_at.elapsed().as_secs() / 60;
    let msg = config.message.replace("{minutes}", &mins.to_string());
    Some(format!("<system-reminder>{msg}</system-reminder>"))
}

/// Build a progress nudge from declared rules.
///
/// Returns `None` if there are no rules or no tracked tools have been
/// called yet. Otherwise returns an XML `<progress>` block wrapped in
/// `<system-reminder>`.
fn build_progress_nudge(rules: &[ProgressRule], history: &[ChatMessage]) -> Option<String> {
    if rules.is_empty() {
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
                && rules.iter().any(|r| r.tool == name.as_str())
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
    for rule in rules {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(tool: &str, min: u32) -> ProgressRule {
        ProgressRule {
            tool: tool.to_string(),
            min: Some(min),
            nudge: None,
        }
    }

    fn rule_with_nudge(tool: &str, min: Option<u32>, nudge: &str) -> ProgressRule {
        ProgressRule {
            tool: tool.to_string(),
            min,
            nudge: Some(nudge.to_string()),
        }
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
        let rules = vec![ProgressRule {
            tool: "note_write".to_string(),
            min: None,
            nudge: None,
        }];
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
        assert!(build_temporal_nudge(started, None).is_none());
    }

    #[test]
    fn temporal_fires_after_threshold() {
        let config = TemporalConfig {
            after_seconds: 60,
            message: "Been working {minutes} min.".to_string(),
        };
        let started = std::time::Instant::now() - std::time::Duration::from_secs(120);
        let nudge = build_temporal_nudge(started, Some(&config)).unwrap();
        assert!(nudge.contains("Been working 2 min."));
    }

    #[test]
    fn temporal_fires_repeatedly() {
        let config = TemporalConfig {
            after_seconds: 60,
            message: "Wrap up.".to_string(),
        };
        let started = std::time::Instant::now() - std::time::Duration::from_secs(120);
        // Should fire every time past the threshold, not just once
        assert!(build_temporal_nudge(started, Some(&config)).is_some());
        assert!(build_temporal_nudge(started, Some(&config)).is_some());
    }

    #[test]
    fn temporal_skips_before_threshold() {
        let config = TemporalConfig {
            after_seconds: 300,
            message: "Wrap up.".to_string(),
        };
        let started = std::time::Instant::now();
        assert!(build_temporal_nudge(started, Some(&config)).is_none());
    }

    #[test]
    fn context_pressure_none_config_returns_none() {
        let history = vec![ChatMessage {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: "x".repeat(300_000),
            }],
        }];
        assert!(build_context_pressure_reminder(&history, None).is_none());
    }

    #[test]
    fn context_pressure_fires_above_threshold() {
        let config = ContextPressureConfig {
            threshold_chars: 100,
            message: "Context large.".to_string(),
        };
        let history = vec![ChatMessage {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: "x".repeat(200),
            }],
        }];
        let nudge = build_context_pressure_reminder(&history, Some(&config)).unwrap();
        assert!(nudge.contains("Context large."));
    }

    #[test]
    fn context_pressure_fires_once() {
        let config = ContextPressureConfig {
            threshold_chars: 100,
            message: "Context large.".to_string(),
        };
        let history = vec![
            ChatMessage {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: "x".repeat(200),
                }],
            },
            ChatMessage {
                role: Role::System,
                content: vec![ContentBlock::Text {
                    text: "<system-reminder>Context large.</system-reminder>".to_string(),
                }],
            },
        ];
        assert!(build_context_pressure_reminder(&history, Some(&config)).is_none());
    }
}
