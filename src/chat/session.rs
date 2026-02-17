use std::sync::Arc;

use serde::Deserialize;
use serde_json::{Value, json};
use surrealdb::Surreal;
use surrealdb::engine::local::Db;
use surrealdb::sql::Thing;

use crate::config::{self, Config};
use crate::db::{self, DatabaseError};
use crate::prompt::{PromptContext, PromptRenderer};
use crate::providers::{
    ChatMessage, ChatRequest, ContentBlock, Provider, Role, StopReason, provider_for_alias,
};
use crate::tools::{RESPOND_TOOL_NAME, ToolContext, ToolManager, ToolSet, format_todo_injection};

use super::convert::{
    citations_to_values, convert_stored_message_to_provider_message, extract_latest_assistant_text,
    extract_text_content, extract_tool_use_blocks, parse_respond_call, parse_session_thing,
    render_tool_error, resolve_web_cache_url, tool_results_to_values,
};
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
        }
    }

    #[must_use]
    pub fn with_max_tool_iterations(mut self, max_tool_iterations: usize) -> Self {
        self.max_tool_iterations = max_tool_iterations;
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
        let mut iterations = 0usize;
        let mut last_result: Option<ChatResult> = None;

        loop {
            let prompt = self.prompt_renderer.render_system_prompt(&PromptContext {
                model: model.clone(),
                provider: self.provider.name().to_string(),
            })?;
            let request = ChatRequest {
                model: model.clone(),
                messages: history.clone(),
                tools: Some(self.tool_manager.all_tool_schemas()),
                max_tokens: None,
                temperature: None,
                system: Some(prompt),
            };
            let response = self.provider.chat(request).await?;

            match response.stop_reason {
                StopReason::ToolUse => {
                    if iterations >= self.max_tool_iterations {
                        let fallback = last_result.unwrap_or_else(|| ChatResult {
                            message: "Hit tool iteration limit before completing response."
                                .to_string(),
                            citations: Vec::new(),
                            stop_reason: ChatStopReason::MaxIterations,
                        });
                        return Ok(ChatResult {
                            stop_reason: ChatStopReason::MaxIterations,
                            ..fallback
                        });
                    }
                    iterations += 1;

                    let tool_uses = extract_tool_use_blocks(&response.content);

                    // Intercept the `respond` tool — it signals the final answer.
                    if let Some((message, citations)) =
                        parse_respond_call(RESPOND_TOOL_NAME, &tool_uses)
                    {
                        let mut citations = citations;
                        self.resolve_citation_urls(&mut citations);
                        let message_id = db::sessions::create_message_with_metadata(
                            &self.db,
                            &session_thing,
                            "assistant",
                            &message,
                            Some(tool_uses),
                            None,
                            Some(citations_to_values(&citations)),
                        )
                        .await?;
                        self.create_citation_edges(&message_id, &citations).await?;

                        let result = ChatResult {
                            message,
                            citations,
                            stop_reason: ChatStopReason::EndTurn,
                        };
                        logfire::info!(
                            "chat complete (respond tool)",
                            session_id = session_id.to_string(),
                            iterations = iterations as u64,
                            citation_count = result.citations.len() as u64,
                            response_len = result.message.len() as u64,
                        );
                        return Ok(result);
                    }

                    let assistant_text = extract_text_content(&response.content);
                    let assistant_msg = ChatMessage {
                        role: Role::Assistant,
                        content: response.content,
                    };
                    db::sessions::create_message_with_metadata(
                        &self.db,
                        &session_thing,
                        "assistant",
                        &assistant_text,
                        Some(tool_uses.clone()),
                        None,
                        None,
                    )
                    .await?;

                    let tool_results = self.execute_tool_calls(session_id, &tool_uses).await;
                    db::sessions::create_message_with_metadata(
                        &self.db,
                        &session_thing,
                        "user",
                        "",
                        None,
                        Some(tool_results_to_values(&tool_results)),
                        None,
                    )
                    .await?;

                    history.push(assistant_msg);
                    history.push(ChatMessage {
                        role: Role::User,
                        content: tool_results,
                    });
                    self.apply_masking_if_needed(&mut history);
                }
                StopReason::EndTurn | StopReason::MaxTokens => {
                    // Fallback: model returned plain text without calling respond.
                    let message = extract_text_content(&response.content);

                    let message_id = db::sessions::create_message_with_metadata(
                        &self.db,
                        &session_thing,
                        "assistant",
                        &message,
                        Some(extract_tool_use_blocks(&response.content)),
                        None,
                        None,
                    )
                    .await?;
                    let _ = message_id; // no citations to link

                    let result = ChatResult {
                        message,
                        citations: Vec::new(),
                        stop_reason: if response.stop_reason == StopReason::MaxTokens {
                            ChatStopReason::MaxTokens
                        } else {
                            ChatStopReason::EndTurn
                        },
                    };
                    logfire::info!(
                        "chat complete",
                        session_id = session_id.to_string(),
                        iterations = iterations as u64,
                        stop_reason = format!("{:?}", result.stop_reason),
                        citation_count = result.citations.len() as u64,
                        response_len = result.message.len() as u64,
                    );
                    return Ok(result);
                }
            }

            if let Some(todo_context) = self.todo_injection_message(&session_thing).await? {
                history.push(ChatMessage {
                    role: Role::System,
                    content: vec![ContentBlock::Text { text: todo_context }],
                });
            }
            last_result = Some(ChatResult {
                message: extract_latest_assistant_text(&history),
                citations: Vec::new(),
                stop_reason: ChatStopReason::EndTurn,
            });
        }
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
        let mut transcript_lines = vec![format!("[job:{job_name}] {prompt}")];
        let mut iterations = 0usize;

        let result = loop {
            let request = ChatRequest {
                model: model.clone(),
                messages: history.clone(),
                tools: Some(self.tool_manager.all_tool_schemas()),
                max_tokens: None,
                temperature: None,
                system: Some(format!("Job: {job_name}")),
            };
            let response = self.provider.chat(request).await?;

            match response.stop_reason {
                StopReason::ToolUse => {
                    if iterations >= self.max_tool_iterations {
                        break ChatResult {
                            message: "Hit tool iteration limit before completing response."
                                .to_string(),
                            citations: Vec::new(),
                            stop_reason: ChatStopReason::MaxIterations,
                        };
                    }
                    iterations += 1;

                    let tool_uses = extract_tool_use_blocks(&response.content);

                    // Intercept the `respond` tool — it signals the final answer.
                    if let Some((message, mut citations)) =
                        parse_respond_call(RESPOND_TOOL_NAME, &tool_uses)
                    {
                        self.resolve_citation_urls(&mut citations);
                        transcript_lines.push(format!("[assistant] {message}"));
                        break ChatResult {
                            message,
                            citations,
                            stop_reason: ChatStopReason::EndTurn,
                        };
                    }

                    let assistant_text = extract_text_content(&response.content);
                    if !assistant_text.is_empty() {
                        transcript_lines.push(format!("[assistant] {assistant_text}"));
                    }
                    history.push(ChatMessage {
                        role: Role::Assistant,
                        content: response.content,
                    });
                    let tool_results = self.execute_tool_calls(session_id, &tool_uses).await;
                    transcript_lines.push(format!(
                        "[tools] {}",
                        serde_json::to_string(&tool_results_to_values(&tool_results))
                            .unwrap_or_else(|_| "[]".to_string())
                    ));
                    history.push(ChatMessage {
                        role: Role::User,
                        content: tool_results,
                    });
                }
                StopReason::EndTurn | StopReason::MaxTokens => {
                    // Fallback: model returned plain text without calling respond.
                    let message = extract_text_content(&response.content);
                    transcript_lines.push(format!("[assistant] {message}"));
                    break ChatResult {
                        message,
                        citations: Vec::new(),
                        stop_reason: if response.stop_reason == StopReason::MaxTokens {
                            ChatStopReason::MaxTokens
                        } else {
                            ChatStopReason::EndTurn
                        },
                    };
                }
            }
        };

        let transcript = transcript_lines.join("\n");
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
    async fn todo_injection_message(
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

    fn resolve_citation_urls(&self, citations: &mut [Citation]) {
        for citation in citations.iter_mut() {
            if citation.url.is_none() && citation.source.starts_with(".web-cache/") {
                citation.url = resolve_web_cache_url(&self.config.workspace, &citation.source);
            }
            if citation.url.is_none() && citation.source.starts_with("http") {
                citation.url = Some(citation.source.clone());
            }
        }
    }

    async fn execute_tool_calls(
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
    async fn create_citation_edges(
        &self,
        message_id: &Thing,
        citations: &[Citation],
    ) -> Result<(), ChatError> {
        for citation in citations {
            if let Some(target) = self.lookup_citation_target(&citation.source).await? {
                self.db
                    .query("RELATE $message_id->cited->$target SET created_at = time::now()")
                    .bind(("message_id", message_id.clone()))
                    .bind(("target", target))
                    .await
                    .map_err(|source| DatabaseError::Query {
                        table: "cited",
                        operation: "relate_message_to_source",
                        source,
                    })?;
            }
        }
        Ok(())
    }

    #[tracing::instrument(skip_all, level = "debug", fields(source = source))]
    async fn lookup_citation_target(&self, source: &str) -> Result<Option<Thing>, ChatError> {
        #[derive(Debug, Deserialize)]
        struct IdRow {
            id: Thing,
        }

        let mut response = self
            .db
            .query("SELECT id FROM reference WHERE path = $path LIMIT 1")
            .bind(("path", source.to_string()))
            .await
            .map_err(|source| DatabaseError::Query {
                table: "reference",
                operation: "lookup_by_path",
                source,
            })?;
        let rows: Vec<IdRow> = response.take(0).map_err(|source| DatabaseError::Query {
            table: "reference",
            operation: "lookup_by_path/take",
            source,
        })?;
        if let Some(row) = rows.first() {
            return Ok(Some(row.id.clone()));
        }

        if source.starts_with(".web-cache/") {
            // TEMPORARY SCAFFOLDING:
            // For spec 06 we materialize web-cache citations as `reference` records.
            // The full knowledge/reference ownership model in spec 13/15 may replace
            // this behavior entirely.
            let url = resolve_web_cache_url(&self.config.workspace, source);
            let mut create = self
                .db
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
                .bind(("source_url", url))
                .await
                .map_err(|source| DatabaseError::Query {
                    table: "reference",
                    operation: "create_web_cache_reference",
                    source,
                })?;
            let created_rows: Vec<IdRow> =
                create.take(0).map_err(|source| DatabaseError::Query {
                    table: "reference",
                    operation: "create_web_cache_reference/take",
                    source,
                })?;
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
}
