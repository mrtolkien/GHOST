use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use surrealdb::Surreal;
use surrealdb::engine::local::Db;
use surrealdb::sql::Thing;

use crate::config::{self, Config};
use crate::db::{self, DatabaseError};
use crate::prompt::{PromptContext, PromptRenderer};
use crate::providers::{
    ChatMessage, ChatRequest, ContentBlock, Provider, ProviderError, ProviderInitError, Role,
    StopReason, provider_for_alias,
};
use crate::tools::{ToolContext, ToolError, ToolManager, ToolSet};

const DEFAULT_MAX_TOOL_ITERATIONS: usize = 25;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct Citation {
    pub source: String,
    pub url: Option<String>,
    pub context: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
pub enum ChatStopReason {
    EndTurn,
    MaxTokens,
    MaxIterations,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ChatResult {
    pub message: String,
    pub citations: Vec<Citation>,
    pub stop_reason: ChatStopReason,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JobTranscript {
    pub transcript: String,
    pub result: ChatResult,
}

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

#[derive(Debug, thiserror::Error)]
pub enum ChatError {
    #[error(transparent)]
    Database(#[from] DatabaseError),

    #[error(transparent)]
    Config(#[from] config::ConfigError),

    #[error(transparent)]
    Provider(#[from] ProviderError),

    #[error(transparent)]
    ProviderInit(#[from] ProviderInitError),

    #[error("invalid session id '{session_id}'")]
    InvalidSessionId { session_id: String },

    #[error("failed to parse structured response json: {0}")]
    InvalidStructuredResponse(#[from] serde_json::Error),
}

#[derive(Debug, Deserialize)]
struct StructuredResponse {
    message: String,
    #[serde(default)]
    citations: Vec<StructuredCitation>,
}

#[derive(Debug, Deserialize)]
struct StructuredCitation {
    source: String,
    context: Option<String>,
}

impl SessionChat {
    #[tracing::instrument(skip_all)]
    pub fn from_config(db: Surreal<Db>, config: Config) -> Result<Self, ChatError> {
        let provider = provider_for_alias(&config, None)?;

        Ok(Self::new(db, provider, ToolManager::new(), config))
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

        let mut history = self.load_provider_history(&session_thing).await?;
        if let Some(todo_context) = self.todo_injection_message(&session_thing).await? {
            history.push(ChatMessage {
                role: Role::System,
                content: vec![ContentBlock::Text(todo_context)],
            });
        }

        let model = self.default_model_name()?;
        let mut iterations = 0usize;
        let mut last_result: Option<ChatResult> = None;

        loop {
            let prompt = self.prompt_renderer.render_system_prompt(&PromptContext {
                model: model.clone(),
                provider: self.provider.name().to_string(),
            });
            let request = ChatRequest {
                model: model.clone(),
                messages: history.clone(),
                tools: Some(self.tool_manager.all_tool_schemas(ToolSet::Chat)),
                max_tokens: None,
                temperature: None,
                system: Some(prompt),
                response_format: Some(citation_response_format()),
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

                    let tool_results = self
                        .execute_tool_calls(session_id, &tool_uses, ToolSet::Chat)
                        .await;
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
                }
                StopReason::EndTurn | StopReason::MaxTokens => {
                    let (message, mut citations) = parse_structured_or_fallback(&response.content);
                    for citation in &mut citations {
                        if citation.url.is_none() && citation.source.starts_with(".web-cache/") {
                            citation.url =
                                resolve_web_cache_url(&self.config.workspace, &citation.source);
                        }
                        if citation.url.is_none() && citation.source.starts_with("http") {
                            citation.url = Some(citation.source.clone());
                        }
                    }

                    let message_id = db::sessions::create_message_with_metadata(
                        &self.db,
                        &session_thing,
                        "assistant",
                        &message,
                        Some(extract_tool_use_blocks(&response.content)),
                        None,
                        Some(citations_to_values(&citations)),
                    )
                    .await?;
                    self.create_citation_edges(&message_id, &citations).await?;

                    let result = ChatResult {
                        message,
                        citations,
                        stop_reason: if response.stop_reason == StopReason::MaxTokens {
                            ChatStopReason::MaxTokens
                        } else {
                            ChatStopReason::EndTurn
                        },
                    };
                    return Ok(result);
                }
            }

            if let Some(todo_context) = self.todo_injection_message(&session_thing).await? {
                history.push(ChatMessage {
                    role: Role::System,
                    content: vec![ContentBlock::Text(todo_context)],
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
        tool_set: ToolSet,
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
            content: vec![ContentBlock::Text(prompt.to_string())],
        }];
        let mut transcript_lines = vec![format!("[job:{job_name}] {prompt}")];
        let mut iterations = 0usize;

        let result = loop {
            let request = ChatRequest {
                model: model.clone(),
                messages: history.clone(),
                tools: Some(self.tool_manager.all_tool_schemas(tool_set.clone())),
                max_tokens: None,
                temperature: None,
                system: Some(format!("Job: {job_name}")),
                response_format: Some(citation_response_format()),
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
                    let assistant_text = extract_text_content(&response.content);
                    if !assistant_text.is_empty() {
                        transcript_lines.push(format!("[assistant] {assistant_text}"));
                    }
                    history.push(ChatMessage {
                        role: Role::Assistant,
                        content: response.content,
                    });
                    let tool_results = self
                        .execute_tool_calls(session_id, &tool_uses, tool_set.clone())
                        .await;
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
                    let (message, mut citations) = parse_structured_or_fallback(&response.content);
                    for citation in &mut citations {
                        if citation.url.is_none() && citation.source.starts_with(".web-cache/") {
                            citation.url =
                                resolve_web_cache_url(&self.config.workspace, &citation.source);
                        }
                        if citation.url.is_none() && citation.source.starts_with("http") {
                            citation.url = Some(citation.source.clone());
                        }
                    }
                    transcript_lines.push(format!("[assistant] {message}"));
                    break ChatResult {
                        message,
                        citations,
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

    #[tracing::instrument(skip_all, fields(session_id = %session_id))]
    async fn load_provider_history(
        &self,
        session_id: &Thing,
    ) -> Result<Vec<ChatMessage>, ChatError> {
        let session = db::sessions::get_session(&self.db, session_id).await?;
        let all_messages = db::sessions::list_messages_by_session(&self.db, session_id).await?;

        let mut messages = Vec::new();
        if let Some(summary) = session.compaction_summary
            && !summary.trim().is_empty()
        {
            messages.push(ChatMessage {
                role: Role::System,
                content: vec![ContentBlock::Text(summary)],
            });
        }

        let cursor = session.compaction_cursor_id;
        let mut include = cursor.is_none();
        for msg in all_messages {
            if !include {
                include = Some(msg.id.to_string()) == cursor;
                continue;
            }
            messages.push(convert_stored_message_to_provider_message(msg));
        }

        Ok(messages)
    }

    #[tracing::instrument(skip_all, fields(session_id = %session_id))]
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
        Ok(Some(format!(
            "Current TODO:\n{}",
            items
                .iter()
                .enumerate()
                .map(|(index, item)| format!("{}. {}", index + 1, item))
                .collect::<Vec<_>>()
                .join("\n")
        )))
    }

    async fn execute_tool_calls(
        &self,
        session_id: &str,
        tool_calls: &[Value],
        tool_set: ToolSet,
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
            let tool_result = self
                .execute_single_tool(session_id, name, id, input, tool_set.clone())
                .await;
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
        _tool_set: ToolSet,
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

    #[tracing::instrument(skip_all, fields(message_id = %message_id))]
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

    #[tracing::instrument(skip_all, fields(source = source))]
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

    fn default_model_name(&self) -> Result<String, ChatError> {
        let alias = &self.config.models.default;
        let model = self.config.models.aliases.get(alias).ok_or_else(|| {
            ChatError::Config(config::ConfigError::UnknownDefaultModelAlias {
                alias: alias.clone(),
            })
        })?;
        Ok(model.model.clone())
    }
}

fn render_tool_error(error: ToolError) -> String {
    match error {
        ToolError::NotFound { name } => format!("Tool not found: {name}"),
        ToolError::ExecutionFailed { name, message } => format!("Tool {name} failed: {message}"),
    }
}

fn parse_session_thing(session_id: &str) -> Result<Thing, ChatError> {
    if session_id.contains(':') {
        let mut parts = session_id.splitn(2, ':');
        let table = parts.next().unwrap_or_default();
        let id = parts.next().unwrap_or_default();
        if table.is_empty() || id.is_empty() {
            return Err(ChatError::InvalidSessionId {
                session_id: session_id.to_string(),
            });
        }
        return Ok(Thing::from((table, id)));
    }

    if session_id.trim().is_empty() {
        return Err(ChatError::InvalidSessionId {
            session_id: session_id.to_string(),
        });
    }

    Ok(Thing::from(("session", session_id)))
}

fn convert_stored_message_to_provider_message(message: db::sessions::MessageRecord) -> ChatMessage {
    let role = match message.role.as_str() {
        "user" => Role::User,
        "assistant" => Role::Assistant,
        _ => Role::System,
    };
    let mut content = Vec::new();
    if !message.content.trim().is_empty() {
        content.push(ContentBlock::Text(message.content));
    }
    if let Some(tool_calls) = message.tool_calls {
        for call in tool_calls {
            if let (Some(id), Some(name)) = (
                call.get("id").and_then(Value::as_str),
                call.get("name").and_then(Value::as_str),
            ) {
                content.push(ContentBlock::ToolUse {
                    id: id.to_string(),
                    name: name.to_string(),
                    input: call.get("input").cloned().unwrap_or_else(|| json!({})),
                });
            }
        }
    }
    if let Some(tool_results) = message.tool_results {
        for result in tool_results {
            if let Some(tool_use_id) = result.get("tool_use_id").and_then(Value::as_str) {
                content.push(ContentBlock::ToolResult {
                    tool_use_id: tool_use_id.to_string(),
                    content: result
                        .get("content")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    is_error: result
                        .get("is_error")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                });
            }
        }
    }
    ChatMessage { role, content }
}

fn extract_tool_use_blocks(content: &[ContentBlock]) -> Vec<Value> {
    content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::ToolUse { id, name, input } => Some(json!({
                "id": id,
                "name": name,
                "input": input
            })),
            _ => None,
        })
        .collect()
}

fn extract_text_content(content: &[ContentBlock]) -> String {
    content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text(text) => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn extract_latest_assistant_text(history: &[ChatMessage]) -> String {
    history
        .iter()
        .rev()
        .find_map(|msg| {
            if msg.role == Role::Assistant {
                Some(extract_text_content(&msg.content))
            } else {
                None
            }
        })
        .unwrap_or_default()
}

fn parse_structured_or_fallback(content: &[ContentBlock]) -> (String, Vec<Citation>) {
    let text = extract_text_content(content);
    let parsed = serde_json::from_str::<StructuredResponse>(&text);
    match parsed {
        Ok(structured) => (
            structured.message,
            structured
                .citations
                .into_iter()
                .map(|citation| Citation {
                    source: citation.source,
                    url: None,
                    context: citation.context,
                })
                .collect(),
        ),
        Err(_) => (text, Vec::new()),
    }
}

fn citations_to_values(citations: &[Citation]) -> Vec<Value> {
    citations
        .iter()
        .map(|citation| {
            json!({
                "source": citation.source,
                "url": citation.url,
                "context": citation.context
            })
        })
        .collect()
}

fn tool_results_to_values(results: &[ContentBlock]) -> Vec<Value> {
    results
        .iter()
        .filter_map(|result| match result {
            ContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => Some(json!({
                "tool_use_id": tool_use_id,
                "content": content,
                "is_error": is_error
            })),
            _ => None,
        })
        .collect()
}

fn citation_response_format() -> crate::providers::ResponseFormat {
    crate::providers::ResponseFormat::JsonSchema {
        name: "ghost_citation_response".to_string(),
        schema: json!({
            "type": "object",
            "properties": {
                "message": { "type": "string", "description": "The response to the OPERATOR" },
                "citations": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "source": { "type": "string", "description": "File path or URL" },
                            "context": { "type": "string", "description": "What this source was used for" }
                        },
                        "required": ["source"]
                    }
                }
            },
            "required": ["message", "citations"]
        }),
    }
}

fn resolve_web_cache_url(workspace: &Path, source: &str) -> Option<String> {
    let path = workspace.join(source);
    let content = std::fs::read_to_string(path).ok()?;
    let mut lines = content.lines();
    if lines.next()? != "---" {
        return None;
    }
    for line in lines {
        if line == "---" {
            break;
        }
        if let Some(value) = line.strip_prefix("url:") {
            let url = value.trim().trim_matches('"');
            if !url.is_empty() {
                return Some(url.to_string());
            }
        }
        if let Some(value) = line.strip_prefix("source_url:") {
            let url = value.trim().trim_matches('"');
            if !url.is_empty() {
                return Some(url.to_string());
            }
        }
    }
    None
}
