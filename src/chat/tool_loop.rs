use async_trait::async_trait;
use serde_json::Value;

use super::interrupt::InterruptReceiver;
use super::tool_cap;
use crate::chat::compaction;
use crate::chat::interrupt::Interrupt;
use crate::providers::types::{ChatResponse, DebugContext, ProviderError, ReasoningEffort};
use crate::providers::{ChatMessage, ChatRequest, ContentBlock, Role, StopReason};

use super::convert::{
    extract_latest_assistant_text, extract_text_content, extract_tool_use_blocks,
    raw_output_to_values,
};
use super::session::SessionChat;
use super::types::{
    ChatError, ChatResult, ChatStopReason, EventSender, RunMetadata, ToolCallInfo, ToolLoopEvent,
    ToolResultInfo,
};
use crate::constants::PROVIDER_REQUEST_TIMEOUT;
use crate::tools::display;

/// Delay before retrying a failed provider request.
const RETRY_DELAY: std::time::Duration = std::time::Duration::from_secs(2);

/// Handler for tool loop events.
///
/// Implementations customize how the loop records assistant messages, tool
/// results, and final answers — e.g. persisting to DB (interactive chat)
/// vs appending to a transcript (background jobs).
#[async_trait]
pub(super) trait ToolLoopHandler: Send {
    fn system_prompt(&self) -> Result<String, ChatError>;

    async fn on_assistant_tool_use(
        &mut self,
        text: &str,
        tool_uses: &[Value],
        raw_output: Option<Vec<Value>>,
    ) -> Result<(), ChatError>;

    async fn on_tool_results(&mut self, results: &[ContentBlock]) -> Result<(), ChatError>;

    async fn on_end_turn(
        &mut self,
        message: String,
        stop_reason: StopReason,
        tool_uses: &[Value],
        raw_output: Option<Vec<Value>>,
    ) -> Result<ChatResult, ChatError>;

    async fn post_tool_iteration(
        &mut self,
        _history: &mut Vec<ChatMessage>,
        _last_input_tokens: u32,
    ) -> Result<(), ChatError> {
        Ok(())
    }

    /// Override the working directory for tool execution.
    /// Returns `None` to use the default (workspace or session cwd_override).
    fn tool_cwd(&self) -> Option<&std::path::Path> {
        None
    }

    /// Check whether the model should be allowed to end its turn.
    ///
    /// Called when the model sends an EndTurn with non-empty text (i.e. tries
    /// to write its final output). Returns `Some(nudge)` if the model should
    /// continue working, `None` if the EndTurn should be accepted.
    async fn check_progress_gate(
        &mut self,
        _history: &[ChatMessage],
    ) -> Result<Option<String>, ChatError> {
        Ok(None)
    }
}

enum InterruptAction {
    Continue,
    Stop,
}

/// Drain all pending interrupts from the channel.
/// Steer messages are persisted to DB and appended to history.
/// Returns `Stop` if any `Interrupt::Stop` was received.
async fn drain_interrupts(
    rx: &mut InterruptReceiver,
    history: &mut Vec<ChatMessage>,
    db: &crate::db::GhostDb,
    session_id: &str,
) -> Result<InterruptAction, ChatError> {
    let mut action = InterruptAction::Continue;
    while let Ok(interrupt) = rx.try_recv() {
        match interrupt {
            Interrupt::Stop => {
                action = InterruptAction::Stop;
            }
            Interrupt::Steer { message } => {
                crate::db::sessions::create_message(db, session_id, "user", &message).await?;
                history.push(ChatMessage {
                    role: Role::User,
                    content: vec![ContentBlock::Text { text: message }],
                });
            }
        }
    }
    Ok(action)
}

/// Contextual channels and identifiers threaded through the tool loop.
pub struct ToolLoopContext<'a> {
    pub event_tx: Option<&'a EventSender>,
    pub interrupt_rx: Option<InterruptReceiver>,
    pub channel_id: Option<String>,
}

/// Model selection and iteration limits for a tool loop run.
pub(super) struct ModelParams<'a> {
    pub model: &'a str,
    pub max_iterations: usize,
    pub reasoning_effort: ReasoningEffort,
}

/// Mutable state carried across iterations of the tool loop.
struct LoopState {
    iterations: usize,
    last_result: Option<ChatResult>,
    retried_empty: bool,
    progress_gate_retries: u8,
    /// Sticky-routing token from the server. Threaded from each response
    /// into the next request so the load-balancer keeps us on the same
    /// server for prompt cache locality.
    turn_state: Option<String>,
    metadata: RunMetadata,
    started_at: std::time::Instant,
}

impl LoopState {
    fn finalize(&mut self) -> RunMetadata {
        self.metadata.iterations = self.iterations;
        self.metadata.duration = self.started_at.elapsed();
        std::mem::take(&mut self.metadata)
    }

    fn accumulate_usage(&mut self, response: &ChatResponse) {
        if response.turn_state.is_some() {
            self.turn_state.clone_from(&response.turn_state);
        }
        self.metadata.input_tokens += response.usage.input_tokens;
        self.metadata.output_tokens += response.usage.output_tokens;
        self.metadata.cache_read_tokens += response.usage.cache_read_tokens.unwrap_or(0);
        self.metadata.cache_creation_tokens += response.usage.cache_creation_tokens.unwrap_or(0);
    }
}

/// Whether the main loop should continue iterating or return a result.
enum IterationOutcome {
    Continue,
    Return(ChatResult),
}

/// Shared tool-use loop for both interactive chat and background jobs.
pub(super) async fn run_tool_loop(
    session_chat: &SessionChat,
    session_id: &str,
    params: ModelParams<'_>,
    handler: &mut (impl ToolLoopHandler + ?Sized),
    history: &mut Vec<ChatMessage>,
    mut ctx: ToolLoopContext<'_>,
) -> Result<(ChatResult, RunMetadata), ChatError> {
    let mut state = LoopState {
        iterations: 0,
        last_result: None,
        retried_empty: false,
        progress_gate_retries: 0,
        turn_state: None,
        metadata: RunMetadata {
            model_alias: session_chat.config().models.default.clone(),
            ..Default::default()
        },
        started_at: std::time::Instant::now(),
    };

    loop {
        // Pre-send: compact if we'd exceed the context window.
        {
            let tools = session_chat.tool_manager().all_tool_schemas();
            let budget = compaction::compute_budget(
                session_chat.model_context_window(),
                "",
                &tools,
                history,
                session_chat.compaction_config().threshold,
            );
            if budget.needs_compaction {
                tracing::warn!(
                    total = budget.total_estimated as u64,
                    window = budget.context_window as u64,
                    threshold = session_chat.compaction_config().threshold,
                    "pre-send compaction triggered — history near context limit",
                );
                handler.post_tool_iteration(history, 0).await?;
            }
        }

        let request = build_request(session_chat, session_id, &params, handler, history, &state)?;
        let response =
            send_with_retry(session_chat, request, handler, history, state.iterations).await?;
        state.accumulate_usage(&response);

        let outcome = match response.stop_reason {
            StopReason::ToolUse => {
                // Check iteration limit before executing tools.
                if state.iterations >= params.max_iterations {
                    let fallback = state.last_result.take().unwrap_or(ChatResult {
                        message: "Hit tool iteration limit before completing response.".to_string(),
                        stop_reason: ChatStopReason::MaxIterations,
                    });
                    IterationOutcome::Return(ChatResult {
                        stop_reason: ChatStopReason::MaxIterations,
                        ..fallback
                    })
                } else {
                    process_tool_use(
                        session_chat,
                        session_id,
                        handler,
                        history,
                        response,
                        &mut state,
                        &mut ctx,
                    )
                    .await?
                }
            }
            StopReason::EndTurn | StopReason::MaxTokens => {
                process_end_turn(
                    session_chat,
                    session_id,
                    handler,
                    history,
                    response,
                    &mut state,
                    &mut ctx,
                )
                .await?
            }
        };

        match outcome {
            IterationOutcome::Return(result) => {
                let metadata = state.finalize();
                return Ok((result, metadata));
            }
            IterationOutcome::Continue => {
                state.last_result = Some(ChatResult {
                    message: extract_latest_assistant_text(history),
                    stop_reason: ChatStopReason::EndTurn,
                });
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Extracted helpers
// ---------------------------------------------------------------------------

fn build_request(
    session_chat: &SessionChat,
    session_id: &str,
    params: &ModelParams<'_>,
    handler: &(impl ToolLoopHandler + ?Sized),
    history: &[ChatMessage],
    state: &LoopState,
) -> Result<ChatRequest, ChatError> {
    let prompt = handler.system_prompt()?;
    Ok(ChatRequest {
        model: params.model.to_string(),
        messages: history.to_vec(),
        tools: {
            let schemas = session_chat.tool_manager().all_tool_schemas();
            if schemas.is_empty() {
                None
            } else {
                Some(schemas)
            }
        },
        max_tokens: None,
        temperature: None,
        system: Some(prompt),
        reasoning_effort: Some(params.reasoning_effort),
        cache_key: session_id.to_string(),
        turn_state: state.turn_state.clone(),
        debug_context: Some(DebugContext {
            session_id: session_id.to_string(),
            iteration: state.iterations,
        }),
        text_verbosity: session_chat
            .model_config()
            .and_then(|m| m.text_verbosity.clone()),
    })
}

/// Send a chat request, retrying once on transient failures.
async fn send_with_retry(
    session_chat: &SessionChat,
    request: ChatRequest,
    handler: &mut (impl ToolLoopHandler + ?Sized),
    history: &mut Vec<ChatMessage>,
    iteration: usize,
) -> Result<ChatResponse, ChatError> {
    let result = tokio::time::timeout(
        PROVIDER_REQUEST_TIMEOUT,
        session_chat.provider().chat(request.clone()),
    )
    .await;

    match result {
        Ok(Ok(resp)) => Ok(resp),
        Ok(Err(ProviderError::ServerError { status, message })) => {
            tracing::warn!(
                status,
                message = message.clone(),
                iteration = iteration as u64,
                "provider returned server error, retrying once",
            );
            tokio::time::sleep(RETRY_DELAY).await;
            retry_once(session_chat, request).await
        }
        Ok(Err(ProviderError::ContextOverflow(msg))) => {
            tracing::warn!(
                error = msg.as_str(),
                iteration = iteration as u64,
                "context overflow — forcing compaction and retrying",
            );
            handler.post_tool_iteration(history, 0).await?;
            let retry_request = ChatRequest {
                messages: history.clone(),
                ..request
            };
            retry_once(session_chat, retry_request).await
        }
        Ok(Err(e)) => Err(ChatError::from(e)),
        Err(_elapsed) => {
            tracing::warn!(
                timeout_secs = PROVIDER_REQUEST_TIMEOUT.as_secs(),
                iteration = iteration as u64,
                "provider request timed out, retrying once",
            );
            retry_once(session_chat, request).await
        }
    }
}

/// Single retry with timeout. Used by all retry paths in `send_with_retry`.
async fn retry_once(
    session_chat: &SessionChat,
    request: ChatRequest,
) -> Result<ChatResponse, ChatError> {
    match tokio::time::timeout(
        PROVIDER_REQUEST_TIMEOUT,
        session_chat.provider().chat(request),
    )
    .await
    {
        Ok(result) => Ok(result.map_err(ChatError::from)?),
        Err(_elapsed) => Err(ChatError::Provider(ProviderError::Timeout {
            seconds: PROVIDER_REQUEST_TIMEOUT.as_secs(),
        })),
    }
}

/// Handle a ToolUse response: execute tools, emit events, check interrupts.
async fn process_tool_use(
    session_chat: &SessionChat,
    session_id: &str,
    handler: &mut (impl ToolLoopHandler + ?Sized),
    history: &mut Vec<ChatMessage>,
    response: ChatResponse,
    state: &mut LoopState,
    ctx: &mut ToolLoopContext<'_>,
) -> Result<IterationOutcome, ChatError> {
    let config = session_chat.config();
    let tool_uses = extract_tool_use_blocks(&response.content);
    let raw_output = raw_output_to_values(&response.content);

    // Count tool calls and collect info for events.
    let tool_infos = collect_tool_call_infos(&tool_uses, &config.workspace);
    for info in &tool_infos {
        *state
            .metadata
            .tool_counts
            .entry(info.name.clone())
            .or_default() += 1;
    }
    if let Some(tx) = ctx.event_tx {
        let _ = tx.send(ToolLoopEvent::ToolCalls { calls: tool_infos });
    }

    // Model made tool calls — reset the empty-response flag.
    state.retried_empty = false;
    state.iterations += 1;

    let assistant_text = extract_text_content(&response.content);
    handler
        .on_assistant_tool_use(&assistant_text, &tool_uses, raw_output)
        .await?;

    history.push(ChatMessage {
        role: Role::Assistant,
        content: response.content,
    });

    // Execute and cap tool results.
    let tool_results = session_chat
        .execute_tool_calls(
            session_id,
            &tool_uses,
            handler.tool_cwd(),
            ctx.channel_id.as_deref(),
        )
        .await;
    let tool_results = tool_cap::cap_content_blocks(
        tool_results,
        &config.workspace,
        config.compaction.max_tool_result_bytes,
    )
    .await;

    handler.on_tool_results(&tool_results).await?;

    // Emit tool result events.
    if let Some(tx) = ctx.event_tx {
        let result_infos = collect_tool_result_infos(&tool_uses, &tool_results);
        if !result_infos.is_empty() {
            let _ = tx.send(ToolLoopEvent::ToolResults {
                results: result_infos,
            });
        }
    }

    // Check for terminal tool calls — end the agent run immediately.
    let any_terminal = tool_uses.iter().any(|call| {
        let name = call.get("name").and_then(Value::as_str).unwrap_or("");
        session_chat.tool_manager().is_terminal(name)
    });
    if any_terminal {
        let terminal_result = extract_terminal_tool_result(&tool_results);
        let result = handler
            .on_end_turn(terminal_result, StopReason::EndTurn, &[], None)
            .await?;
        return Ok(IterationOutcome::Return(result));
    }

    history.push(ChatMessage {
        role: Role::User,
        content: tool_results,
    });

    handler
        .post_tool_iteration(history, response.usage.input_tokens)
        .await?;

    // Check for OPERATOR interrupts (steering messages or /stop).
    if let Some(ref mut rx) = ctx.interrupt_rx
        && let InterruptAction::Stop =
            drain_interrupts(rx, history, session_chat.db(), session_id).await?
    {
        let fallback = state.last_result.take().unwrap_or(ChatResult {
            message: String::new(),
            stop_reason: ChatStopReason::Stopped,
        });
        return Ok(IterationOutcome::Return(ChatResult {
            stop_reason: ChatStopReason::Stopped,
            ..fallback
        }));
    }

    Ok(IterationOutcome::Continue)
}

/// Handle an EndTurn/MaxTokens response: empty-response recovery, progress
/// gating, final interrupt drain.
async fn process_end_turn(
    session_chat: &SessionChat,
    session_id: &str,
    handler: &mut (impl ToolLoopHandler + ?Sized),
    history: &mut Vec<ChatMessage>,
    response: ChatResponse,
    state: &mut LoopState,
    ctx: &mut ToolLoopContext<'_>,
) -> Result<IterationOutcome, ChatError> {
    let tool_uses = extract_tool_use_blocks(&response.content);
    let message = extract_text_content(&response.content);

    // Detect empty EndTurn: no text and no tool calls.
    if message.is_empty() && tool_uses.is_empty() {
        return handle_empty_response(session_chat.db(), session_id, history, &response, state)
            .await;
    }

    // Progress gate: does the handler want the model to keep working?
    // Max 3 retries to break "commitment loop" patterns.
    if state.progress_gate_retries < 3
        && let Some(nudge) = handler.check_progress_gate(history).await?
    {
        tracing::warn!(
            iterations = state.iterations as u64,
            response_len = message.len() as u64,
            gate_retry = state.progress_gate_retries as u64,
            "progress gate triggered — model tried to end \
             prematurely, injecting continuation nudge",
        );
        history.push(ChatMessage {
            role: Role::System,
            content: vec![ContentBlock::Text { text: nudge }],
        });
        state.progress_gate_retries += 1;
        state.iterations += 1;
        return Ok(IterationOutcome::Continue);
    }

    let raw_output = raw_output_to_values(&response.content);
    let result = handler
        .on_end_turn(message, response.stop_reason, &tool_uses, raw_output)
        .await?;

    // Drain pending interrupts. If a user message arrived during the
    // final LLM call, persist it and continue so the model responds.
    if let Some(ref mut rx) = ctx.interrupt_rx {
        let pre_drain_len = history.len();
        match drain_interrupts(rx, history, session_chat.db(), session_id).await? {
            InterruptAction::Stop => {
                return Ok(IterationOutcome::Return(result));
            }
            InterruptAction::Continue => {
                if history.len() > pre_drain_len {
                    history.push(ChatMessage {
                        role: Role::Assistant,
                        content: response.content,
                    });
                    state.last_result = None;
                    state.iterations += 1;
                    return Ok(IterationOutcome::Continue);
                }
            }
        }
    }

    tracing::info!(
        iterations = state.iterations as u64,
        stop_reason = format!("{:?}", result.stop_reason),
        response_len = result.message.len() as u64,
        response = &result.message,
        "agent run complete",
    );
    Ok(IterationOutcome::Return(result))
}

/// Handle an empty response — nudge once, error on second occurrence.
async fn handle_empty_response(
    db: &crate::db::GhostDb,
    session_id: &str,
    history: &mut Vec<ChatMessage>,
    response: &ChatResponse,
    state: &mut LoopState,
) -> Result<IterationOutcome, ChatError> {
    let content_json = serde_json::to_string(&response.content)
        .unwrap_or_else(|e| format!("<serialization failed: {e}>"));

    if !state.retried_empty {
        let nudge_text = "<system-reminder>You returned an empty \
                          response. This is a bug — your session \
                          will end if it happens again. Continue \
                          by making a tool call now.</system-reminder>";

        tracing::warn!(
            iterations = state.iterations as u64,
            stop_reason = format!("{:?}", response.stop_reason),
            content = content_json,
            "empty EndTurn response, injecting recovery nudge",
        );

        // Persist to DB so compaction IDs stay in sync with in-memory history.
        crate::db::sessions::create_message(db, session_id, "system", nudge_text).await?;

        history.push(ChatMessage {
            role: Role::System,
            content: vec![ContentBlock::Text {
                text: nudge_text.to_string(),
            }],
        });
        state.retried_empty = true;
        return Ok(IterationOutcome::Continue);
    }

    tracing::error!(
        iterations = state.iterations as u64,
        stop_reason = format!("{:?}", response.stop_reason),
        content = content_json,
        "empty EndTurn response after recovery nudge",
    );
    Err(ChatError::Provider(ProviderError::EmptyResponse {
        detail: "provider returned empty EndTurn twice".to_string(),
    }))
}

// ---------------------------------------------------------------------------
// Pure data transformations for event emission
// ---------------------------------------------------------------------------

fn collect_tool_call_infos(tool_uses: &[Value], workspace: &std::path::Path) -> Vec<ToolCallInfo> {
    tool_uses
        .iter()
        .filter_map(|t| {
            let name = t.get("name").and_then(Value::as_str)?;
            let input = t.get("input").unwrap_or(&Value::Null);
            Some(ToolCallInfo {
                name: name.to_string(),
                args_summary: summarize_tool_args(name, input, workspace),
                display: display::display_request(name, input),
            })
        })
        .collect()
}

fn collect_tool_result_infos(
    tool_uses: &[Value],
    tool_results: &[ContentBlock],
) -> Vec<ToolResultInfo> {
    tool_results
        .iter()
        .filter_map(|block| {
            let ContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } = block
            else {
                return None;
            };
            let call = tool_uses
                .iter()
                .find(|t| t.get("id").and_then(Value::as_str) == Some(tool_use_id))?;
            let name = call.get("name").and_then(Value::as_str)?;
            let input = call.get("input").unwrap_or(&Value::Null);
            Some(ToolResultInfo {
                name: name.to_string(),
                display_request: display::display_request(name, input),
                display_result: display::display_result(name, input, content, *is_error),
            })
        })
        .collect()
}

/// Extract the text result from the first terminal tool's result block.
fn extract_terminal_tool_result(tool_results: &[ContentBlock]) -> String {
    for block in tool_results {
        if let ContentBlock::ToolResult { content, .. } = block {
            return content.clone();
        }
    }
    String::new()
}

const ARG_VALUE_MAX: usize = 60;
const ARGS_SUMMARY_MAX: usize = 120;

/// Produce a short human-readable summary of tool call arguments.
fn summarize_tool_args(tool_name: &str, input: &Value, workspace: &std::path::Path) -> String {
    let Some(obj) = input.as_object() else {
        return String::new();
    };
    if obj.is_empty() {
        return String::new();
    }

    let mut parts = Vec::new();
    let mut total = 0usize;

    for (key, val) in obj {
        if is_default_arg(tool_name, key, val, workspace) {
            continue;
        }
        let val_str = match val {
            Value::String(s) => truncate_str(s, ARG_VALUE_MAX),
            Value::Null => "null".to_string(),
            Value::Bool(b) => b.to_string(),
            Value::Number(n) => n.to_string(),
            other => {
                let json = other.to_string();
                truncate_str(&json, ARG_VALUE_MAX)
            }
        };
        let part = format!("{key}: {val_str}");
        total += part.len();
        parts.push(part);
        if total > ARGS_SUMMARY_MAX {
            break;
        }
    }

    let result = parts.join(", ");
    truncate_str(&result, ARGS_SUMMARY_MAX)
}

/// Returns true if the argument matches a known default value for the tool.
fn is_default_arg(tool_name: &str, key: &str, val: &Value, workspace: &std::path::Path) -> bool {
    match (tool_name, key) {
        ("shell", "background") | ("web_fetch", "scroll") => val == &Value::Bool(false),
        ("shell", "timeout_ms") => val.as_u64() == Some(30_000),
        ("shell", "directory") => val
            .as_str()
            .is_some_and(|s| matches!(s, "" | ".") || std::path::Path::new(s) == workspace),
        ("knowledge_search", "limit") => val.as_u64() == Some(10),
        _ => false,
    }
}

fn truncate_str(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max - 1).collect();
        format!("{truncated}\u{2026}")
    }
}
