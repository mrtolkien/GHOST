use std::sync::Arc;

use crate::db::GhostDb;
use async_trait::async_trait;
use serde_json::{Value, json};

use crate::config::{self, Config, SharedConfig, SharedConfigExt};
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
    convert_stored_message_to_provider_message, images_to_values, parse_session_thing,
    render_tool_error, tool_results_to_values,
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
    config: SharedConfig,
    prompt_renderer: PromptRenderer,
    max_tool_iterations: usize,
    agent_runner: Option<Arc<crate::agents::AgentRunner>>,
    compaction_override: Option<config::CompactionConfig>,
    event_tx: Option<crate::events::SessionEventSender>,
    cwd_override: Option<std::path::PathBuf>,
    active_sessions: super::interrupt::ActiveSessions,
    confirmation_tx: Option<crate::tools::confirmation::ConfirmationSender>,
    browser_manager: Arc<tokio::sync::Mutex<crate::web::browser::BrowserManager>>,
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
    pub fn from_config(db: GhostDb, config: SharedConfig) -> Result<Self, ChatError> {
        let cfg = config.current();
        let provider = provider_for_alias(&cfg, None)?;
        let mut tool_manager = ToolManager::for_chat();
        tool_manager.with_browser();

        Ok(Self::new(db, provider, tool_manager, config))
    }

    #[must_use]
    pub fn new(
        db: GhostDb,
        provider: Arc<dyn Provider>,
        tool_manager: ToolManager,
        config: SharedConfig,
    ) -> Self {
        let cfg = config.current();
        let prompt_renderer = PromptRenderer::new(config.clone());
        let browser_manager = Arc::new(tokio::sync::Mutex::new(
            crate::web::browser::BrowserManager::new(cfg.web.browsers.clone()),
        ));
        Self {
            db,
            provider,
            tool_manager,
            config,
            prompt_renderer,
            max_tool_iterations: DEFAULT_MAX_TOOL_ITERATIONS,
            agent_runner: None,
            compaction_override: None,
            event_tx: None,
            cwd_override: None,
            active_sessions: std::sync::Arc::new(dashmap::DashMap::new()),
            confirmation_tx: None,
            browser_manager,
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
    pub fn with_event_sender(mut self, tx: crate::events::SessionEventSender) -> Self {
        self.event_tx = Some(tx);
        self
    }

    #[must_use]
    pub fn with_cwd_override(mut self, cwd: std::path::PathBuf) -> Self {
        self.cwd_override = Some(cwd);
        self
    }

    #[must_use]
    pub fn with_confirmation_tx(
        mut self,
        tx: crate::tools::confirmation::ConfirmationSender,
    ) -> Self {
        self.confirmation_tx = Some(tx);
        self
    }

    #[must_use]
    pub fn with_active_sessions(
        mut self,
        active_sessions: super::interrupt::ActiveSessions,
    ) -> Self {
        self.active_sessions = active_sessions;
        self
    }

    pub fn active_sessions(&self) -> &super::interrupt::ActiveSessions {
        &self.active_sessions
    }

    /// Return the effective compaction config (override if set, else global).
    ///
    /// When no override is set, returns the compaction config from the current
    /// config snapshot. The returned owned value avoids lifetime issues with
    /// the `Arc<Config>` temporary.
    pub(super) fn compaction_config(&self) -> config::CompactionConfig {
        self.compaction_override
            .clone()
            .unwrap_or_else(|| self.config.current().compaction.clone())
    }

    #[tracing::instrument(name = "orchestrate response", skip_all, fields(session_id = session_id))]
    pub async fn chat(
        &self,
        session_id: &str,
        user_message: &str,
        channel_id: Option<String>,
        event_tx: Option<&EventSender>,
    ) -> Result<(ChatResult, RunMetadata), ChatError> {
        self.chat_with_images(session_id, user_message, None, channel_id, event_tx)
            .await
    }

    pub async fn chat_with_images(
        &self,
        session_id: &str,
        user_message: &str,
        images: Option<Vec<ContentBlock>>,
        channel_id: Option<String>,
        event_tx: Option<&EventSender>,
    ) -> Result<(ChatResult, RunMetadata), ChatError> {
        let session_thing = parse_session_thing(session_id)?;
        db::sessions::get_session(&self.db, &session_thing).await?;
        db::sessions::update_activity(&self.db, &session_thing).await?;

        // Atomic session guard — prevent concurrent tool loops.
        let (int_tx, int_rx) = super::interrupt::channel();
        {
            use dashmap::mapref::entry::Entry;
            match self.active_sessions.entry(session_id.to_string()) {
                Entry::Occupied(_) => {
                    return Err(ChatError::SessionBusy {
                        session_id: session_id.to_string(),
                    });
                }
                Entry::Vacant(entry) => {
                    entry.insert(int_tx);
                }
            }
        }

        let image_values = images.as_ref().and_then(|imgs| {
            let vals: Vec<serde_json::Value> = imgs
                .iter()
                .filter_map(|b| match b {
                    ContentBlock::Image {
                        path,
                        mime_type,
                        filename,
                    } => Some(serde_json::json!({
                        "path": path,
                        "mime_type": mime_type,
                        "filename": filename,
                    })),
                    _ => None,
                })
                .collect();
            if vals.is_empty() { None } else { Some(vals) }
        });
        db::sessions::create_message_with_metadata(
            &self.db,
            &session_thing,
            "user",
            user_message,
            None,
            None,
            None,
            image_values,
        )
        .await?;

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

        let result = run_tool_loop(
            self,
            session_id,
            &model,
            self.max_tool_iterations,
            effort,
            &mut handler,
            &mut history,
            event_tx,
            Some(int_rx),
            channel_id,
        )
        .await;

        self.active_sessions.remove(session_id);
        result
    }

    /// Chat in a coding session with a custom system prompt and working directory.
    #[tracing::instrument(name = "orchestrate coding response", skip_all, fields(session_id = session_id))]
    pub async fn chat_coding(
        &self,
        session_id: &str,
        user_message: &str,
        system_prompt: &str,
        working_dir: &std::path::Path,
        channel_id: Option<String>,
        event_tx: Option<&EventSender>,
    ) -> Result<(ChatResult, RunMetadata), ChatError> {
        let session_thing = parse_session_thing(session_id)?;
        db::sessions::get_session(&self.db, &session_thing).await?;
        db::sessions::update_activity(&self.db, &session_thing).await?;

        // Atomic session guard — prevent concurrent tool loops.
        let (int_tx, int_rx) = super::interrupt::channel();
        {
            use dashmap::mapref::entry::Entry;
            match self.active_sessions.entry(session_id.to_string()) {
                Entry::Occupied(_) => {
                    return Err(ChatError::SessionBusy {
                        session_id: session_id.to_string(),
                    });
                }
                Entry::Vacant(entry) => {
                    entry.insert(int_tx);
                }
            }
        }

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
            working_dir: working_dir.to_path_buf(),
            compaction: coding_compaction_config(&self.compaction_config()),
        };

        let result = run_tool_loop(
            self,
            session_id,
            &model,
            self.max_tool_iterations,
            effort,
            &mut handler,
            &mut history,
            event_tx,
            Some(int_rx),
            channel_id,
        )
        .await;

        self.active_sessions.remove(session_id);
        result
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

        // Collect cursor-filtered records before conversion so we can
        // repair orphaned tool calls at the DB level first.
        let cursor = session.compaction_cursor_id;
        let mut include = cursor.is_none();
        let mut filtered: Vec<db::sessions::MessageRecord> = Vec::new();
        for msg in all_messages {
            if !include {
                include = Some(msg.id.clone()) == cursor;
                continue;
            }
            filtered.push(msg);
        }

        self.repair_orphaned_tool_calls(session_id, &mut filtered)
            .await?;

        for msg in filtered {
            let msg_id = msg.id.clone();
            messages.push(convert_stored_message_to_provider_message(msg));
            ids.push(msg_id);
        }

        Ok((messages, ids))
    }

    /// Find assistant messages whose tool calls lack corresponding tool
    /// results in the following message. For each orphan, persist an error
    /// tool-result message to the DB and insert it right after the
    /// assistant message.
    ///
    /// Handles both fully missing results (crash before any results were
    /// written) and partially missing results (crash mid-execution where
    /// some results were written but not all).
    async fn repair_orphaned_tool_calls(
        &self,
        session_id: &str,
        messages: &mut Vec<db::sessions::MessageRecord>,
    ) -> Result<(), ChatError> {
        // (insert_at_index, record_to_insert)
        let mut insertions: Vec<(usize, db::sessions::MessageRecord)> = Vec::new();

        for i in 0..messages.len() {
            if messages[i].role != "assistant" {
                continue;
            }
            let tool_calls = match messages[i].tool_calls_parsed() {
                Some(tc) if !tc.is_empty() => tc,
                _ => continue,
            };

            let call_ids: Vec<String> = tool_calls
                .iter()
                .filter_map(|c| c.get("id").and_then(Value::as_str).map(String::from))
                .collect();

            // Collect tool-result IDs from all following non-assistant messages.
            // Repairs from prior runs may not be at i+1 (their DB timestamp
            // placed them later), so we scan forward until the next assistant
            // message to find all answered IDs.
            let mut answered_ids = std::collections::HashSet::<String>::new();
            let mut last_result_idx = i; // track last message with results
            for (j, msg) in messages.iter().enumerate().skip(i + 1) {
                if msg.role == "assistant" {
                    break;
                }
                if let Some(results) = msg.tool_results_parsed() {
                    for r in &results {
                        if let Some(id) = r.get("tool_use_id").and_then(Value::as_str) {
                            answered_ids.insert(id.to_string());
                        }
                    }
                    last_result_idx = j;
                }
            }

            let orphaned_ids: Vec<&str> = call_ids
                .iter()
                .filter(|id| !answered_ids.contains(id.as_str()))
                .map(String::as_str)
                .collect();

            if orphaned_ids.is_empty() {
                continue;
            }

            let error_results: Vec<Value> = orphaned_ids
                .iter()
                .map(|id| {
                    json!({
                        "tool_use_id": id,
                        "content": "Tool execution was interrupted \
                            (host crashed or restarted before the tool \
                            could return a result). You may retry.",
                        "is_error": true,
                    })
                })
                .collect();

            // Use a timestamp just after the assistant message (or last
            // partial-result message) so the repair sorts correctly on
            // future DB loads instead of landing at the end of the history.
            let anchor = &messages[last_result_idx].created_at;
            let repair_ts = bump_timestamp(anchor);

            let msg_id = db::sessions::create_message_with_timestamp(
                &self.db,
                session_id,
                "user",
                "",
                None,
                Some(error_results.clone()),
                None,
                None,
                &repair_ts,
            )
            .await?;

            let repair_record = db::sessions::MessageRecord {
                id: msg_id,
                session_id: session_id.to_string(),
                role: "user".to_string(),
                content: String::new(),
                tool_calls: None,
                tool_results: Some(serde_json::to_string(&error_results).unwrap_or_default()),
                raw_output: None,
                images: None,
                created_at: repair_ts,
            };

            // Insert right after the last result message (or the assistant).
            let insert_at = last_result_idx + 1;
            insertions.push((insert_at, repair_record));

            let sid = session_id.to_string();
            tracing::warn!(
                session_id = sid,
                orphaned_count = orphaned_ids.len(),
                message_index = i,
                "repaired orphaned tool calls",
            );
        }

        // Insert in reverse order so earlier indices remain valid.
        for (idx, record) in insertions.into_iter().rev() {
            messages.insert(idx, record);
        }

        Ok(())
    }

    #[tracing::instrument(name = "run tools", skip_all)]
    pub(super) async fn execute_tool_calls(
        &self,
        session_id: &str,
        tool_calls: &[Value],
        cwd_override: Option<&std::path::Path>,
        channel_id: Option<&str>,
    ) -> Vec<ContentBlock> {
        let futures: Vec<_> = tool_calls
            .iter()
            .filter_map(|call| {
                let id = call.get("id").and_then(Value::as_str)?;
                let name = call.get("name").and_then(Value::as_str)?;
                let input = call.get("input").cloned().unwrap_or_else(|| json!({}));
                Some(self.execute_single_tool(
                    session_id,
                    name,
                    id,
                    input,
                    cwd_override,
                    channel_id,
                ))
            })
            .collect();

        futures::future::join_all(futures)
            .await
            .into_iter()
            .flatten()
            .collect()
    }

    async fn execute_single_tool(
        &self,
        session_id: &str,
        name: &str,
        tool_use_id: &str,
        input: Value,
        cwd_override: Option<&std::path::Path>,
        channel_id: Option<&str>,
    ) -> Vec<ContentBlock> {
        let config = self.config.current();
        let cwd = cwd_override
            .map(|p| p.to_path_buf())
            .or_else(|| self.cwd_override.clone())
            .unwrap_or_else(|| config.workspace.clone());
        let tool_ctx = ToolContext {
            workspace: config.workspace.clone(),
            cwd,
            db: self.db.clone(),
            config,
            session_id: session_id.to_string(),
            agent_runner: self.agent_runner.clone(),
            event_tx: self.event_tx.clone(),
            channel_id: channel_id.map(String::from),
            confirmation_tx: self.confirmation_tx.clone(),
            browser_manager: self.browser_manager.clone(),
        };

        match self.tool_manager.execute(name, input, &tool_ctx).await {
            Ok(output) => {
                let mut blocks = vec![ContentBlock::ToolResult {
                    tool_use_id: tool_use_id.to_string(),
                    content: output.text,
                    is_error: false,
                }];
                for img in output.images {
                    blocks.push(ContentBlock::Image {
                        path: img.path,
                        mime_type: img.mime_type,
                        filename: img.filename,
                    });
                }
                blocks
            }
            Err(error) => vec![ContentBlock::ToolResult {
                tool_use_id: tool_use_id.to_string(),
                content: render_tool_error(error),
                is_error: true,
            }],
        }
    }

    pub(super) fn default_model_name(&self) -> Result<String, ChatError> {
        let config = self.config.current();
        let alias = &config.models.default;
        let model = config.models.aliases.get(alias).ok_or_else(|| {
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

    pub fn config(&self) -> Arc<Config> {
        self.config.current()
    }

    /// The resolved config for the default model alias.
    pub(super) fn model_config(&self) -> Option<crate::config::ModelConfig> {
        let config = self.config.current();
        config.models.aliases.get(&config.models.default).cloned()
    }

    /// Context window size (in tokens) from the default model alias.
    pub(super) fn model_context_window(&self) -> usize {
        self.model_config()
            .map(|m| m.context_window as usize)
            .unwrap_or(200_000)
    }

    /// Reasoning effort configured on the default model alias, if any.
    fn model_reasoning_effort(&self) -> Option<ReasoningEffort> {
        let config = self.config.current();
        config
            .models
            .aliases
            .get(&config.models.default)
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
            None,
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
            images_to_values(results),
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
        let msg_id = db::sessions::create_message_with_metadata(
            self.session_chat.db(),
            self.session_thing,
            "assistant",
            &message,
            Some(tool_uses.to_vec()),
            None,
            raw_output,
            None,
        )
        .await?;

        // Extract citations and create message_source records
        let citations = super::citations::extract_citations(&message);
        for citation in &citations {
            let _ = db::knowledge::create_message_source(
                self.session_chat.db(),
                &msg_id,
                &citation.url,
                citation.title.as_deref(),
            )
            .await;
        }

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
        let compacted = self
            .session_chat
            .compact_in_tool_loop(self.session_thing, history)
            .await;
        if compacted
            && let Some(tx) = self.event_tx
        {
            let _ = tx.send(ToolLoopEvent::Compacted);
        }

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

/// Build a compaction config tuned for coding sessions.
///
/// Uses coding-specific instructions that preserve plan/TODO state, files
/// modified, test results, and OPERATOR decisions across compaction boundaries.
fn coding_compaction_config(base: &config::CompactionConfig) -> config::CompactionConfig {
    config::CompactionConfig {
        instructions: Some(
            "Preserve the following across compaction:\n\
             - The current plan and TODO checklist status\n\
             - All files created, modified, or deleted\n\
             - Test results (pass/fail, which tests)\n\
             - OPERATOR decisions and preferences\n\
             - Current git branch and recent commits\n\
             - Any errors or blockers encountered"
                .to_string(),
        ),
        ..base.clone()
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
    working_dir: std::path::PathBuf,
    compaction: config::CompactionConfig,
}

#[async_trait]
impl ToolLoopHandler for CodingHandler<'_> {
    fn system_prompt(&self) -> Result<String, ChatError> {
        Ok(self.system_prompt.clone())
    }

    fn tool_cwd(&self) -> Option<&std::path::Path> {
        Some(&self.working_dir)
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
            None,
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
            images_to_values(results),
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
            None,
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
        let compacted = self
            .session_chat
            .compact_in_tool_loop_with_config(self.session_thing, history, &self.compaction)
            .await;
        if compacted
            && let Some(tx) = self.event_tx
        {
            let _ = tx.send(ToolLoopEvent::Compacted);
        }

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
            None,
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
            images_to_values(results),
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
            None,
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
        let compacted = self
            .session_chat
            .compact_in_tool_loop(self.session_thing, history)
            .await;
        if compacted
            && let Some(tx) = self.event_tx
        {
            let _ = tx.send(ToolLoopEvent::Compacted);
        }

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
                    tracing::warn!(
                        error = e.to_string(),
                        agent = self.config.name.clone(),
                        "lua pre_turn hook error",
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
                tracing::warn!(
                    error = e.to_string(),
                    agent = self.config.name.clone(),
                    "lua on_end_turn hook error",
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
        channel_id: Option<String>,
        event_tx: Option<&EventSender>,
    ) -> Result<(ChatResult, RunMetadata), ChatError> {
        let session_thing = parse_session_thing(session_id)?;

        // Persist initial messages to DB and build provider history
        let mut history = Vec::new();
        for msg in &build_result.messages {
            let has_tool_metadata = msg.tool_calls.is_some() || msg.tool_results.is_some();

            if has_tool_metadata {
                db::sessions::create_message_with_metadata(
                    &self.db,
                    &session_thing,
                    &msg.role,
                    &msg.content,
                    msg.tool_calls.clone(),
                    msg.tool_results.clone(),
                    None,
                    None,
                )
                .await?;
            } else {
                db::sessions::create_message(&self.db, &session_thing, &msg.role, &msg.content)
                    .await?;
            }

            let role = match msg.role.as_str() {
                "assistant" => Role::Assistant,
                "system" => Role::System,
                _ => Role::User,
            };

            // Build content blocks — include tool_use/tool_result blocks when present
            let mut content = Vec::new();
            if !msg.content.is_empty() {
                content.push(ContentBlock::Text {
                    text: msg.content.clone(),
                });
            }
            if let Some(ref tool_calls) = msg.tool_calls {
                for tc in tool_calls {
                    if let (Some(id), Some(name)) = (
                        tc.get("id").and_then(|v| v.as_str()),
                        tc.get("name").and_then(|v| v.as_str()),
                    ) {
                        content.push(ContentBlock::ToolUse {
                            id: id.to_string(),
                            name: name.to_string(),
                            input: tc.get("input").cloned().unwrap_or(serde_json::Value::Null),
                        });
                    }
                }
            }
            if let Some(ref tool_results) = msg.tool_results {
                for tr in tool_results {
                    if let Some(tool_use_id) = tr.get("tool_use_id").and_then(|v| v.as_str()) {
                        content.push(ContentBlock::ToolResult {
                            tool_use_id: tool_use_id.to_string(),
                            content: tr
                                .get("content")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                            is_error: tr
                                .get("is_error")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false),
                        });
                    }
                }
            }

            history.push(ChatMessage { role, content });
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

        let (int_tx, int_rx) = super::interrupt::channel();
        self.active_sessions.insert(session_id.to_string(), int_tx);

        let result = run_tool_loop(
            self,
            session_id,
            &model,
            config.max_iterations,
            effort,
            &mut handler,
            &mut history,
            event_tx,
            Some(int_rx),
            channel_id,
        )
        .await;

        self.active_sessions.remove(session_id);
        result
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
        channel_id: Option<String>,
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
        self.compact_in_tool_loop(&session_thing, &mut history)
            .await;

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

        let (int_tx, int_rx) = super::interrupt::channel();
        self.active_sessions.insert(session_id.to_string(), int_tx);

        let result = run_tool_loop(
            self,
            session_id,
            &model,
            config.max_iterations,
            effort,
            &mut handler,
            &mut history,
            event_tx,
            Some(int_rx),
            channel_id,
        )
        .await;

        self.active_sessions.remove(session_id);
        result
    }
}

/// Advance an RFC 3339 timestamp by 1 millisecond so a repair message sorts
/// right after its anchor message in `ORDER BY created_at ASC` queries.
fn bump_timestamp(ts: &str) -> String {
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(ts) {
        (dt + chrono::Duration::milliseconds(1)).to_rfc3339()
    } else {
        // Fallback: append a 'z' so it lexicographically sorts just after.
        format!("{ts}+")
    }
}
