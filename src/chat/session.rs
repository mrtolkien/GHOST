use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};
use surrealdb::Surreal;
use surrealdb::engine::local::Db;
use surrealdb::sql::Thing;

use crate::config::{self, Config};
use crate::db;
use crate::prompt::{PromptContext, PromptRenderer};
use crate::providers::{ChatMessage, ContentBlock, Provider, Role, StopReason, provider_for_alias};
use crate::tools::{ToolContext, ToolManager, ToolSet, format_todo_injection};

use super::convert::{
    citations_to_values, convert_stored_message_to_provider_message, parse_session_thing,
    render_tool_error, resolve_web_cache_url, tool_results_to_values,
};
use super::tool_loop::{ToolLoopHandler, run_tool_loop};
use super::types::{
    ChatError, ChatResult, ChatStopReason, Citation, DEFAULT_MAX_TOOL_ITERATIONS, JobTranscript,
};

pub struct SessionChat {
    db: Surreal<Db>,
    provider: Arc<dyn Provider>,
    tool_manager: ToolManager,
    config: Config,
    prompt_renderer: PromptRenderer,
    max_tool_iterations: usize,
    agent_runner: Option<Arc<crate::agents::AgentRunner>>,
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
    #[tracing::instrument(skip_all)]
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
            agent_runner: None,
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

    #[tracing::instrument(skip_all, fields(session_id = session_id))]
    pub async fn chat(
        &self,
        session_id: &str,
        user_message: &str,
    ) -> Result<ChatResult, ChatError> {
        let session_thing = parse_session_thing(session_id)?;
        db::sessions::get_session(&self.db, &session_thing).await?;
        db::sessions::update_activity(&self.db, &session_thing).await?;
        db::sessions::create_message(&self.db, &session_thing, "user", user_message).await?;

        let (mut history, stored_ids) = self.load_provider_history(&session_thing).await?;
        self.compact_if_needed(&session_thing, &mut history, &stored_ids)
            .await;

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
        };

        run_tool_loop(
            self,
            session_id,
            &model,
            self.max_tool_iterations,
            &mut handler,
            &mut history,
        )
        .await
    }

    #[tracing::instrument(skip_all, fields(job_name = job_name, session_id = session_id))]
    pub async fn chat_job(
        &self,
        job_name: &str,
        session_id: &str,
        prompt: &str,
        _tool_set: ToolSet,
    ) -> Result<JobTranscript, ChatError> {
        // TEMPORARY SCAFFOLDING:
        // This is a minimal spec 06 implementation. Jobs spec 16/17 is expected to
        // redesign transcript shape, status handling, and storage boundaries.
        let session_thing = parse_session_thing(session_id)?;
        let job_log_id =
            db::job_logs::create_running_job_log(&self.db, job_name, "job", Some(&session_thing))
                .await?;

        let model = self.default_model_name()?;
        let mut history = vec![ChatMessage {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: prompt.to_string(),
            }],
        }];

        let mut handler = JobHandler {
            job_name: job_name.to_string(),
            transcript_lines: vec![format!("[job:{job_name}] {prompt}")],
        };

        let result = run_tool_loop(
            self,
            session_id,
            &model,
            self.max_tool_iterations,
            &mut handler,
            &mut history,
        )
        .await?;

        let transcript = handler.transcript_lines.join("\n");
        db::job_logs::finish_job_log(
            &self.db,
            &job_log_id,
            if result.stop_reason == ChatStopReason::MaxIterations {
                "failed"
            } else {
                "ok"
            },
            &transcript,
        )
        .await?;

        Ok(JobTranscript { transcript, result })
    }

    /// Run an agent tool loop with a custom system prompt.
    ///
    /// Messages are persisted to the agent's own session. Returns the final
    /// assistant message.
    #[tracing::instrument(skip_all, fields(
        agent_name = agent_name,
        session_id = session_id
    ))]
    pub async fn chat_agent(
        &self,
        agent_name: &str,
        session_id: &str,
        prompt: &str,
        system_prompt: String,
        max_iterations: usize,
    ) -> Result<ChatResult, ChatError> {
        let session_thing = parse_session_thing(session_id)?;
        db::sessions::create_message(&self.db, &session_thing, "user", prompt).await?;

        let model = self.default_model_name()?;
        let mut history = vec![ChatMessage {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: prompt.to_string(),
            }],
        }];

        let mut handler = AgentHandler {
            session_chat: self,
            session_thing: &session_thing,
            system_prompt,
            agent_name: agent_name.to_string(),
        };

        run_tool_loop(
            self,
            session_id,
            &model,
            max_iterations,
            &mut handler,
            &mut history,
        )
        .await
    }

    #[tracing::instrument(skip_all, fields(old_session_id = session_id))]
    pub async fn reboot_session(&self, session_id: &str) -> Result<String, ChatError> {
        let old_session = parse_session_thing(session_id)?;
        db::sessions::mark_rebooted(&self.db, &old_session).await?;
        let new_session = db::sessions::create_session(&self.db).await?;
        db::interface_sessions::replace_session_everywhere(&self.db, &old_session, &new_session)
            .await?;
        Ok(new_session.to_string())
    }

    /// Load provider history, returning `(messages, stored_message_ids)`.
    ///
    /// `stored_message_ids` is parallel to the returned messages (one DB
    /// message ID per provider message). The summary pseudo-message (if any)
    /// gets an empty string as its ID.
    #[tracing::instrument(skip_all, level = "debug", fields(session_id = %session_id))]
    pub(super) async fn load_provider_history(
        &self,
        session_id: &Thing,
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
                include = Some(msg.id.to_string()) == cursor;
                continue;
            }
            let msg_id = msg.id.to_string();
            messages.push(convert_stored_message_to_provider_message(msg));
            ids.push(msg_id);
        }

        Ok((messages, ids))
    }

    #[tracing::instrument(skip_all, level = "debug", fields(session_id = %session_id))]
    pub(super) async fn todo_injection_message(
        &self,
        session_id: &Thing,
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

    pub(super) fn resolve_citation_urls(&self, citations: &mut [Citation]) {
        for citation in citations.iter_mut() {
            if citation.url.is_none() && citation.source.starts_with(".web-cache/") {
                citation.url = resolve_web_cache_url(&self.config.workspace, &citation.source);
            }
            if citation.url.is_none() && citation.source.starts_with("http") {
                citation.url = Some(citation.source.clone());
            }
        }
    }

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
            agent_runner: self.agent_runner.clone(),
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

    #[tracing::instrument(skip_all, level = "debug", fields(message_id = %message_id))]
    pub(super) async fn create_citation_edges(
        &self,
        message_id: &Thing,
        citations: &[Citation],
    ) -> Result<(), ChatError> {
        use crate::db::query::query_exec;

        for citation in citations {
            if let Some(target) = self.lookup_citation_target(&citation.source).await? {
                query_exec(
                    self.db
                        .query(
                            "RELATE $message_id->cited->$target \
                             SET created_at = time::now()",
                        )
                        .bind(("message_id", message_id.clone()))
                        .bind(("target", target)),
                    "cited",
                    "relate_message_to_source",
                )
                .await?;
            }
        }
        Ok(())
    }

    #[tracing::instrument(skip_all, level = "debug", fields(source = source))]
    async fn lookup_citation_target(&self, source: &str) -> Result<Option<Thing>, ChatError> {
        use crate::db::query::{IdRow, query_exec, take_many};

        let mut resp = query_exec(
            self.db
                .query("SELECT id FROM reference WHERE path = $path LIMIT 1")
                .bind(("path", source.to_string())),
            "reference",
            "lookup_by_path",
        )
        .await?;
        let rows: Vec<IdRow> = take_many(&mut resp, 0, "reference", "lookup_by_path")?;
        if let Some(row) = rows.first() {
            return Ok(Some(row.id.clone()));
        }

        // Check notes by path (e.g. "notes/rust.md" -> title "Rust")
        if source.starts_with("notes/") {
            let title = std::path::Path::new(source)
                .file_stem()
                .and_then(|s| s.to_str())
                .map(|s| {
                    s.chars()
                        .enumerate()
                        .map(|(i, c)| {
                            if i == 0 || s.as_bytes().get(i - 1) == Some(&b'_') {
                                c.to_uppercase().next().unwrap_or(c)
                            } else if c == '_' {
                                ' '
                            } else {
                                c
                            }
                        })
                        .collect::<String>()
                });
            if let Some(title) = title {
                let mut note_resp = query_exec(
                    self.db
                        .query(
                            "SELECT id FROM note \
                             WHERE title = $title LIMIT 1",
                        )
                        .bind(("title", title)),
                    "note",
                    "lookup_by_title",
                )
                .await?;
                let note_rows: Vec<IdRow> =
                    take_many(&mut note_resp, 0, "note", "lookup_by_title")?;
                if let Some(row) = note_rows.first() {
                    return Ok(Some(row.id.clone()));
                }
            }
        }

        if source.starts_with(".web-cache/") {
            // TEMPORARY SCAFFOLDING:
            // For spec 06 we materialize web-cache citations as `reference`
            // records. The full knowledge/reference ownership model in
            // spec 13/15 may replace this behavior entirely.
            let url = resolve_web_cache_url(&self.config.workspace, source);
            let mut create = query_exec(
                self.db
                    .query(
                        "CREATE reference SET \
                            topic = 'web_cache', \
                            path = $path, \
                            content = '', \
                            source_url = $source_url, \
                            created_at = time::now() \
                         RETURN id",
                    )
                    .bind(("path", source.to_string()))
                    .bind(("source_url", url)),
                "reference",
                "create_web_cache_reference",
            )
            .await?;
            let created_rows: Vec<IdRow> =
                take_many(&mut create, 0, "reference", "create_web_cache_reference")?;
            return Ok(created_rows.first().map(|row| row.id.clone()));
        }

        Ok(None)
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
    session_thing: &'a Thing,
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

    async fn on_respond(
        &mut self,
        message: String,
        citations: Vec<Citation>,
        tool_uses: &[Value],
    ) -> Result<ChatResult, ChatError> {
        // Filter out the respond tool_use — it's a control-flow tool, not a
        // real tool call. Storing it without a matching tool_result would
        // produce an orphaned tool_use that breaks history on session
        // resumption (e.g. after agent completion injects findings).
        let non_respond: Vec<Value> = tool_uses
            .iter()
            .filter(|v| {
                v.get("name").and_then(Value::as_str) != Some(crate::tools::RESPOND_TOOL_NAME)
            })
            .cloned()
            .collect();
        let stored_tool_uses = if non_respond.is_empty() {
            None
        } else {
            Some(non_respond)
        };

        let message_id = db::sessions::create_message_with_metadata(
            self.session_chat.db(),
            self.session_thing,
            "assistant",
            &message,
            stored_tool_uses,
            None,
            Some(citations_to_values(&citations)),
        )
        .await?;
        self.session_chat
            .create_citation_edges(&message_id, &citations)
            .await?;

        Ok(ChatResult {
            message,
            citations,
            stop_reason: ChatStopReason::EndTurn,
        })
    }

    async fn on_assistant_tool_use(
        &mut self,
        text: &str,
        tool_uses: &[Value],
    ) -> Result<(), ChatError> {
        db::sessions::create_message_with_metadata(
            self.session_chat.db(),
            self.session_thing,
            "assistant",
            text,
            Some(tool_uses.to_vec()),
            None,
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
        )
        .await?;
        Ok(())
    }

    async fn on_end_turn(
        &mut self,
        message: String,
        stop_reason: StopReason,
        tool_uses: &[Value],
    ) -> Result<ChatResult, ChatError> {
        db::sessions::create_message_with_metadata(
            self.session_chat.db(),
            self.session_thing,
            "assistant",
            &message,
            Some(tool_uses.to_vec()),
            None,
            None,
        )
        .await?;

        Ok(ChatResult {
            message,
            citations: Vec::new(),
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
        if let Some(todo_context) = self
            .session_chat
            .todo_injection_message(self.session_thing)
            .await?
        {
            history.push(ChatMessage {
                role: Role::System,
                content: vec![ContentBlock::Text { text: todo_context }],
            });
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// AgentHandler — persists to DB, uses agent's system prompt
// ---------------------------------------------------------------------------

struct AgentHandler<'a> {
    session_chat: &'a SessionChat,
    session_thing: &'a Thing,
    system_prompt: String,
    #[allow(dead_code)]
    agent_name: String,
}

#[async_trait]
impl ToolLoopHandler for AgentHandler<'_> {
    fn system_prompt(&self) -> Result<String, ChatError> {
        Ok(self.system_prompt.clone())
    }

    async fn on_respond(
        &mut self,
        message: String,
        citations: Vec<Citation>,
        tool_uses: &[Value],
    ) -> Result<ChatResult, ChatError> {
        db::sessions::create_message_with_metadata(
            self.session_chat.db(),
            self.session_thing,
            "assistant",
            &message,
            Some(tool_uses.to_vec()),
            None,
            Some(citations_to_values(&citations)),
        )
        .await?;

        Ok(ChatResult {
            message,
            citations,
            stop_reason: ChatStopReason::EndTurn,
        })
    }

    async fn on_assistant_tool_use(
        &mut self,
        text: &str,
        tool_uses: &[Value],
    ) -> Result<(), ChatError> {
        db::sessions::create_message_with_metadata(
            self.session_chat.db(),
            self.session_thing,
            "assistant",
            text,
            Some(tool_uses.to_vec()),
            None,
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
        )
        .await?;
        Ok(())
    }

    async fn on_end_turn(
        &mut self,
        message: String,
        stop_reason: StopReason,
        tool_uses: &[Value],
    ) -> Result<ChatResult, ChatError> {
        db::sessions::create_message_with_metadata(
            self.session_chat.db(),
            self.session_thing,
            "assistant",
            &message,
            Some(tool_uses.to_vec()),
            None,
            None,
        )
        .await?;

        Ok(ChatResult {
            message,
            citations: Vec::new(),
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
        if let Some(todo_context) = self
            .session_chat
            .todo_injection_message(self.session_thing)
            .await?
        {
            history.push(ChatMessage {
                role: Role::System,
                content: vec![ContentBlock::Text { text: todo_context }],
            });
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// JobHandler — appends to transcript string
// ---------------------------------------------------------------------------

struct JobHandler {
    job_name: String,
    transcript_lines: Vec<String>,
}

#[async_trait]
impl ToolLoopHandler for JobHandler {
    fn system_prompt(&self) -> Result<String, ChatError> {
        Ok(format!("Job: {}", self.job_name))
    }

    async fn on_respond(
        &mut self,
        message: String,
        citations: Vec<Citation>,
        _tool_uses: &[Value],
    ) -> Result<ChatResult, ChatError> {
        self.transcript_lines.push(format!("[assistant] {message}"));
        Ok(ChatResult {
            message,
            citations,
            stop_reason: ChatStopReason::EndTurn,
        })
    }

    async fn on_assistant_tool_use(
        &mut self,
        text: &str,
        _tool_uses: &[Value],
    ) -> Result<(), ChatError> {
        if !text.is_empty() {
            self.transcript_lines.push(format!("[assistant] {text}"));
        }
        Ok(())
    }

    async fn on_tool_results(&mut self, results: &[ContentBlock]) -> Result<(), ChatError> {
        self.transcript_lines.push(format!(
            "[tools] {}",
            serde_json::to_string(&tool_results_to_values(results))
                .unwrap_or_else(|_| "[]".to_string())
        ));
        Ok(())
    }

    async fn on_end_turn(
        &mut self,
        message: String,
        stop_reason: StopReason,
        _tool_uses: &[Value],
    ) -> Result<ChatResult, ChatError> {
        self.transcript_lines.push(format!("[assistant] {message}"));
        Ok(ChatResult {
            message,
            citations: Vec::new(),
            stop_reason: if stop_reason == StopReason::MaxTokens {
                ChatStopReason::MaxTokens
            } else {
                ChatStopReason::EndTurn
            },
        })
    }
}
