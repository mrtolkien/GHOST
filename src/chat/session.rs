use std::sync::Arc;

use crate::db::GhostDb;
use async_trait::async_trait;
use serde_json::{Value, json};

use crate::config::{self, Config};
use crate::db;
use crate::prompt::{PromptContext, PromptRenderer};
use crate::providers::{
    ChatMessage, ContentBlock, Provider, ReasoningEffort, Role, StopReason, provider_for_alias,
    resolve_reasoning_effort,
};
use crate::scripting::ScriptHost;
use crate::scripting::types::{BuildResult, PreTurnState, TodoSummary};
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
    agent_runner: Option<Arc<crate::agents::AgentRunner>>,
    compaction_override: Option<config::CompactionConfig>,
    completion_tx: Option<crate::completion::CompletionSender>,
    cwd_override: Option<std::path::PathBuf>,
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
            agent_runner: None,
            compaction_override: None,
            completion_tx: None,
            cwd_override: None,
        }
    }

    #[must_use]
    pub fn with_max_tool_iterations(mut self, max_tool_iterations: usize) -> Self {
        self.max_tool_iterations = max_tool_iterations;
        self
    }

    #[must_use]
    pub fn with_agent_runner(mut self, runner: Arc<crate::agents::AgentRunner>) -> Self {
        self.agent_runner = Some(runner);
        self
    }

    #[must_use]
    pub fn with_compaction_config(mut self, compaction: config::CompactionConfig) -> Self {
        self.compaction_override = Some(compaction);
        self
    }

    #[must_use]
    pub fn with_completion_sender(mut self, tx: crate::completion::CompletionSender) -> Self {
        self.completion_tx = Some(tx);
        self
    }

    #[must_use]
    pub fn with_cwd_override(mut self, cwd: std::path::PathBuf) -> Self {
        self.cwd_override = Some(cwd);
        self
    }

    /// Return the effective compaction config (override if set, else global).
    pub(super) fn compaction_config(&self) -> &config::CompactionConfig {
        self.compaction_override
            .as_ref()
            .unwrap_or(&self.config.compaction)
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

    /// Chat in a coding session with a custom system prompt.
    /// Uses `cwd_override` if set, otherwise falls back to workspace.
    #[tracing::instrument(name = "orchestrate coding response", skip_all, fields(session_id = session_id))]
    pub async fn chat_coding(
        &self,
        session_id: &str,
        user_message: &str,
        system_prompt: &str,
        event_tx: Option<&EventSender>,
    ) -> Result<(ChatResult, RunMetadata), ChatError> {
        let session_thing = parse_session_thing(session_id)?;
        db::sessions::get_session(&self.db, &session_thing).await?;
        db::sessions::update_activity(&self.db, &session_thing).await?;
        db::sessions::create_message(&self.db, &session_thing, "user", user_message).await?;

        let (mut history, stored_ids) = self.load_provider_history(&session_thing).await?;
        self.compact_if_needed(&session_thing, &mut history, &stored_ids)
            .await;

        let model = self.default_model_name()?;
        let effort = resolve_reasoning_effort(None, None, self.model_reasoning_effort());
        let mut handler = CodingHandler {
            session_chat: self,
            session_thing: &session_thing,
            event_tx,
            pending_todo_update: false,
            system_prompt: system_prompt.to_string(),
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
        let cwd = self
            .cwd_override
            .clone()
            .unwrap_or_else(|| self.config.workspace.clone());
        let tool_ctx = ToolContext {
            workspace: self.config.workspace.clone(),
            cwd,
            db: self.db.clone(),
            config: self.config.clone(),
            session_id: session_id.to_string(),
            agent_runner: self.agent_runner.clone(),
            completion_tx: self.completion_tx.clone(),
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
    pub(super) fn model_context_window(&self) -> usize {
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

        // Emit TodoUpdated UI event (no injection — main chat has no TODO nudge)
        if self.pending_todo_update {
            let todo_items =
                db::sessions::get_session_todo_list(self.session_chat.db(), self.session_thing)
                    .await?;
            if let Some(ref items) = todo_items
                && let Some(tx) = self.event_tx
            {
                let _ = tx.send(ToolLoopEvent::TodoUpdated {
                    items: items.clone(),
                });
            }
            self.pending_todo_update = false;
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// CodingHandler — coding session handler with custom system prompt
// ---------------------------------------------------------------------------

struct CodingHandler<'a> {
    session_chat: &'a SessionChat,
    session_thing: &'a str,
    event_tx: Option<&'a EventSender>,
    pending_todo_update: bool,
    system_prompt: String,
}

#[async_trait]
impl ToolLoopHandler for CodingHandler<'_> {
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
        _last_input_tokens: u32,
    ) -> Result<(), ChatError> {
        self.session_chat
            .compact_in_tool_loop(self.session_thing, history)
            .await;

        if self.pending_todo_update {
            let todo_items =
                db::sessions::get_session_todo_list(self.session_chat.db(), self.session_thing)
                    .await?;
            if let Some(ref items) = todo_items
                && let Some(tx) = self.event_tx
            {
                let _ = tx.send(ToolLoopEvent::TodoUpdated {
                    items: items.clone(),
                });
            }
            self.pending_todo_update = false;
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// LuaAgentHandler — agent tool loop handler using Lua hooks for nudges and gating
// ---------------------------------------------------------------------------

struct LuaAgentHandler<'a> {
    session_chat: &'a SessionChat,
    session_thing: &'a str,
    system_prompt: String,
    config: &'a crate::scripting::AgentConfig,
    script_host: &'a ScriptHost,
    started_at: std::time::Instant,
    iteration_count: usize,
    last_input_tokens: u32,
    tool_counts: std::collections::HashMap<String, u32>,
    temporal_fire_count: usize,
    context_pressure_fired: bool,
    event_tx: Option<&'a EventSender>,
    pending_todo_update: bool,
}

#[async_trait]
impl ToolLoopHandler for LuaAgentHandler<'_> {
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

        // Accumulate tool usage counts for Lua nudges
        for tool in tool_uses {
            if let Some(name) = tool.get("name").and_then(Value::as_str) {
                *self.tool_counts.entry(name.to_string()).or_insert(0) += 1;
            }
        }

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
        self.session_chat
            .compact_in_tool_loop(self.session_thing, history)
            .await;

        // Fetch TODO items for Lua state and UI events (injection is in Lua nudges)
        let todo_items =
            db::sessions::get_session_todo_list(self.session_chat.db(), self.session_thing).await?;

        if self.pending_todo_update {
            if let Some(ref items) = todo_items
                && let Some(tx) = self.event_tx
            {
                let _ = tx.send(ToolLoopEvent::TodoUpdated {
                    items: items.clone(),
                });
            }
            self.pending_todo_update = false;
        }

        // Call Lua pre_turn hook if present
        if self.config.has_pre_turn {
            let state = self.build_pre_turn_state(&todo_items);
            match self.script_host.call_pre_turn(state) {
                Ok(Some(nudge_result)) => {
                    self.temporal_fire_count += nudge_result.temporal_fired as usize;
                    self.context_pressure_fired |= nudge_result.context_pressure_fired;
                    history.push(ChatMessage {
                        role: Role::System,
                        content: vec![ContentBlock::Text {
                            text: nudge_result.text,
                        }],
                    });
                }
                Ok(None) => {}
                Err(e) => {
                    logfire::warn!(
                        "lua pre_turn hook error",
                        error = e.to_string(),
                        agent = self.config.name.clone(),
                    );
                }
            }
        }

        self.iteration_count += 1;

        Ok(())
    }

    async fn check_progress_gate(
        &mut self,
        _history: &[ChatMessage],
    ) -> Result<Option<String>, ChatError> {
        if !self.config.has_on_end_turn {
            return Ok(None);
        }

        let todo_items =
            db::sessions::get_session_todo_list(self.session_chat.db(), self.session_thing).await?;
        let state = self.build_pre_turn_state(&todo_items);

        match self.script_host.call_on_end_turn(state) {
            Ok(Some(msg)) => Ok(Some(format!("<system-reminder>{msg}</system-reminder>"))),
            Ok(None) => Ok(None),
            Err(e) => {
                logfire::warn!(
                    "lua on_end_turn hook error",
                    error = e.to_string(),
                    agent = self.config.name.clone(),
                );
                Ok(None)
            }
        }
    }
}

impl LuaAgentHandler<'_> {
    fn build_pre_turn_state(
        &self,
        todo_items: &Option<Vec<crate::tools::TodoItem>>,
    ) -> PreTurnState {
        use crate::tools::TodoStatus;

        let todo_summary = todo_items.as_ref().map(|items| {
            let total = items.len();
            let completed = items
                .iter()
                .filter(|i| matches!(i.status, TodoStatus::Done | TodoStatus::Skipped))
                .count();
            TodoSummary {
                total,
                completed,
                incomplete: total.saturating_sub(completed),
            }
        });

        let todo_text = todo_items.as_ref().and_then(|items| {
            if items.is_empty() {
                None
            } else {
                Some(format_todo_injection(items))
            }
        });

        let context_window = self.session_chat.model_context_window();

        PreTurnState {
            iteration: self.iteration_count,
            max_iterations: self.config.max_iterations,
            remaining: self
                .config
                .max_iterations
                .saturating_sub(self.iteration_count),
            elapsed_seconds: self.started_at.elapsed().as_secs(),
            tool_counts: self.tool_counts.clone(),
            last_input_tokens: self.last_input_tokens,
            context_window,
            todo_summary,
            todo_text,
            temporal_fire_count: self.temporal_fire_count,
            context_pressure_fired: self.context_pressure_fired,
        }
    }
}

impl SessionChat {
    /// Run a Lua-defined agent in a fresh session.
    #[tracing::instrument(name = "run lua agent", skip_all, fields(
        gen_ai.agent.name = %config.name,
        session_id = session_id,
    ))]
    pub async fn run_agent(
        &self,
        session_id: &str,
        build_result: BuildResult,
        config: &crate::scripting::AgentConfig,
        script_host: &ScriptHost,
        event_tx: Option<&EventSender>,
    ) -> Result<(ChatResult, RunMetadata), ChatError> {
        let session_thing = parse_session_thing(session_id)?;

        // Persist initial messages to DB and build provider history
        let mut history = Vec::new();
        for msg in &build_result.messages {
            db::sessions::create_message(&self.db, &session_thing, &msg.role, &msg.content).await?;
            let role = match msg.role.as_str() {
                "assistant" => Role::Assistant,
                "system" => Role::System,
                _ => Role::User,
            };
            history.push(ChatMessage {
                role,
                content: vec![ContentBlock::Text {
                    text: msg.content.clone(),
                }],
            });
        }

        let model = self.default_model_name()?;
        let effort =
            resolve_reasoning_effort(None, config.reasoning_effort, self.model_reasoning_effort());

        let mut handler = LuaAgentHandler {
            session_chat: self,
            session_thing: &session_thing,
            system_prompt: build_result.system_prompt,
            config,
            script_host,
            started_at: std::time::Instant::now(),
            iteration_count: 0,
            last_input_tokens: 0,
            tool_counts: std::collections::HashMap::new(),
            temporal_fire_count: 0,
            context_pressure_fired: false,
            event_tx,
            pending_todo_update: false,
        };

        run_tool_loop(
            self,
            session_id,
            &model,
            config.max_iterations,
            effort,
            &mut handler,
            &mut history,
            event_tx,
        )
        .await
    }

    /// Resume a Lua-defined agent with pre-built history.
    ///
    /// `messages` is the full message list (from DB + any modifications by
    /// `on_resume`). `db_message_count` indicates how many messages at the
    /// front are already persisted — only messages beyond that index are
    /// written to DB before entering the tool loop.
    #[allow(clippy::too_many_arguments)]
    #[tracing::instrument(name = "run lua agent with history", skip_all, fields(
        gen_ai.agent.name = %config.name,
        session_id = session_id,
    ))]
    pub async fn run_agent_with_history(
        &self,
        session_id: &str,
        system_prompt: String,
        messages: &[crate::scripting::LuaMessage],
        db_message_count: usize,
        config: &crate::scripting::AgentConfig,
        script_host: &ScriptHost,
        event_tx: Option<&EventSender>,
    ) -> Result<(ChatResult, RunMetadata), ChatError> {
        let session_thing = parse_session_thing(session_id)?;

        // Persist only NEW messages (those beyond db_message_count)
        for msg in messages.iter().skip(db_message_count) {
            db::sessions::create_message(&self.db, &session_thing, &msg.role, &msg.content).await?;
        }

        // Build provider history from the full message list
        let mut history = Vec::new();
        for msg in messages {
            let role = match msg.role.as_str() {
                "assistant" => Role::Assistant,
                "system" => Role::System,
                _ => Role::User,
            };
            history.push(ChatMessage {
                role,
                content: vec![ContentBlock::Text {
                    text: msg.content.clone(),
                }],
            });
        }
        self.apply_masking_if_needed(&mut history);

        let model = self.default_model_name()?;
        let effort =
            resolve_reasoning_effort(None, config.reasoning_effort, self.model_reasoning_effort());

        let mut handler = LuaAgentHandler {
            session_chat: self,
            session_thing: &session_thing,
            system_prompt,
            config,
            script_host,
            started_at: std::time::Instant::now(),
            iteration_count: 0,
            last_input_tokens: 0,
            tool_counts: std::collections::HashMap::new(),
            temporal_fire_count: 0,
            context_pressure_fired: false,
            event_tx,
            pending_todo_update: false,
        };

        run_tool_loop(
            self,
            session_id,
            &model,
            config.max_iterations,
            effort,
            &mut handler,
            &mut history,
            event_tx,
        )
        .await
    }
}
