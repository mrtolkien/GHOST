use std::time::Duration;

use async_trait::async_trait;
use serde_json::Value;

use super::interrupt::InterruptReceiver;
use crate::chat::compaction;
use crate::chat::interrupt::Interrupt;
use crate::providers::types::{DebugContext, ProviderError, ReasoningEffort};
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
use crate::tools::display;

/// Per-request timeout for provider API calls. Providers can hang indefinitely
/// (observed in live tests). This wraps each `Provider::chat()` call.
/// On timeout, the request is retried once before propagating the error.
const PROVIDER_REQUEST_TIMEOUT: Duration = Duration::from_secs(180);

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

/// Shared tool-use loop for both interactive chat and background jobs.
#[allow(clippy::too_many_arguments)]
pub(super) async fn run_tool_loop(
    session_chat: &SessionChat,
    session_id: &str,
    model: &str,
    max_iterations: usize,
    reasoning_effort: ReasoningEffort,
    handler: &mut (impl ToolLoopHandler + ?Sized),
    history: &mut Vec<ChatMessage>,
    event_tx: Option<&EventSender>,
    mut interrupt_rx: Option<InterruptReceiver>,
    channel_id: Option<String>,
) -> Result<(ChatResult, RunMetadata), ChatError> {
    let started_at = std::time::Instant::now();
    let config = session_chat.config();
    let mut metadata = RunMetadata {
        model_alias: config.models.default.clone(),
        ..Default::default()
    };

    let mut iterations = 0usize;
    let mut last_result: Option<ChatResult> = None;
    let mut retried_empty = false;
    let mut progress_gate_retries = 0u8;
    // Sticky-routing token from the server. Threaded from each response
    // into the next request so the load-balancer keeps us on the same
    // server for prompt cache locality.
    let mut turn_state: Option<String> = None;

    loop {
        // Pre-send safety check: estimate token usage and compact if we'd
        // exceed the context window. Normally post-response compaction keeps
        // us under threshold, so this firing indicates a large tool result
        // was appended since the last check.
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

        let prompt = handler.system_prompt()?;
        let request = ChatRequest {
            model: model.to_string(),
            messages: history.clone(),
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
            reasoning_effort: Some(reasoning_effort),
            cache_key: session_id.to_string(),
            turn_state: turn_state.clone(),
            debug_context: Some(DebugContext {
                session_id: session_id.to_string(),
                iteration: iterations,
            }),
            text_verbosity: session_chat
                .model_config()
                .and_then(|m| m.text_verbosity.clone()),
        };
        let response = match tokio::time::timeout(
            PROVIDER_REQUEST_TIMEOUT,
            session_chat.provider().chat(request.clone()),
        )
        .await
        {
            Ok(Ok(resp)) => resp,
            Ok(Err(ProviderError::ServerError { status, message })) => {
                tracing::warn!(
                    status = status,
                    message = message.clone(),
                    iteration = iterations as u64,
                    "provider returned server error, retrying once",
                );
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                match tokio::time::timeout(
                    PROVIDER_REQUEST_TIMEOUT,
                    session_chat.provider().chat(request),
                )
                .await
                {
                    Ok(result) => result.map_err(ChatError::from)?,
                    Err(_elapsed) => {
                        return Err(ChatError::Provider(ProviderError::Timeout {
                            seconds: PROVIDER_REQUEST_TIMEOUT.as_secs(),
                        }));
                    }
                }
            }
            Ok(Err(e)) => return Err(ChatError::from(e)),
            Err(_elapsed) => {
                tracing::warn!(
                    timeout_secs = PROVIDER_REQUEST_TIMEOUT.as_secs(),
                    iteration = iterations as u64,
                    "provider request timed out, retrying once",
                );
                match tokio::time::timeout(
                    PROVIDER_REQUEST_TIMEOUT,
                    session_chat.provider().chat(request),
                )
                .await
                {
                    Ok(result) => result.map_err(ChatError::from)?,
                    Err(_elapsed) => {
                        tracing::error!(
                            timeout_secs = PROVIDER_REQUEST_TIMEOUT.as_secs(),
                            iteration = iterations as u64,
                            "provider request timed out twice",
                        );
                        return Err(ChatError::Provider(ProviderError::Timeout {
                            seconds: PROVIDER_REQUEST_TIMEOUT.as_secs(),
                        }));
                    }
                }
            }
        };

        // Capture sticky-routing token for next iteration.
        if response.turn_state.is_some() {
            turn_state = response.turn_state.clone();
        }

        // Accumulate usage from every provider response
        metadata.input_tokens += response.usage.input_tokens;
        metadata.output_tokens += response.usage.output_tokens;
        metadata.cache_read_tokens += response.usage.cache_read_tokens.unwrap_or(0);

        match response.stop_reason {
            StopReason::ToolUse => {
                if iterations >= max_iterations {
                    metadata.iterations = iterations;
                    metadata.duration = started_at.elapsed();
                    let fallback = last_result.unwrap_or(ChatResult {
                        message: "Hit tool iteration limit before completing response.".to_string(),
                        stop_reason: ChatStopReason::MaxIterations,
                    });
                    return Ok((
                        ChatResult {
                            stop_reason: ChatStopReason::MaxIterations,
                            ..fallback
                        },
                        metadata,
                    ));
                }
                iterations += 1;

                let tool_uses = extract_tool_use_blocks(&response.content);
                let raw_output = raw_output_to_values(&response.content);

                // Count tool calls and collect info for events
                let tool_infos: Vec<ToolCallInfo> = tool_uses
                    .iter()
                    .filter_map(|t| {
                        let name = t.get("name").and_then(Value::as_str)?;
                        let input = t.get("input").unwrap_or(&Value::Null);
                        Some(ToolCallInfo {
                            name: name.to_string(),
                            args_summary: summarize_tool_args(name, input, &config.workspace),
                            display: display::display_request(name, input),
                        })
                    })
                    .collect();
                for info in &tool_infos {
                    *metadata.tool_counts.entry(info.name.clone()).or_default() += 1;
                }

                // Emit tool call event
                if let Some(tx) = event_tx {
                    let _ = tx.send(ToolLoopEvent::ToolCalls { calls: tool_infos });
                }

                // Model made tool calls — reset the empty-response flag
                // so it gets another recovery chance if it goes empty later.
                retried_empty = false;

                let assistant_text = extract_text_content(&response.content);
                handler
                    .on_assistant_tool_use(&assistant_text, &tool_uses, raw_output)
                    .await?;

                history.push(ChatMessage {
                    role: Role::Assistant,
                    content: response.content,
                });

                let tool_results = session_chat
                    .execute_tool_calls(
                        session_id,
                        &tool_uses,
                        handler.tool_cwd(),
                        channel_id.as_deref(),
                    )
                    .await;
                handler.on_tool_results(&tool_results).await?;

                // Emit tool result event for UI editing.
                // Results are flattened (Image blocks interleaved), so match
                // back to tool_uses by tool_use_id rather than zipping.
                if let Some(tx) = event_tx {
                    let result_infos: Vec<ToolResultInfo> = tool_results
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
                            let call = tool_uses.iter().find(|t| {
                                t.get("id").and_then(Value::as_str) == Some(tool_use_id)
                            })?;
                            let name = call.get("name").and_then(Value::as_str)?;
                            let input = call.get("input").unwrap_or(&Value::Null);
                            Some(ToolResultInfo {
                                name: name.to_string(),
                                display_request: display::display_request(name, input),
                                display_result: display::display_result(
                                    name, input, content, *is_error,
                                ),
                            })
                        })
                        .collect();
                    if !result_infos.is_empty() {
                        let _ = tx.send(ToolLoopEvent::ToolResults {
                            results: result_infos,
                        });
                    }
                }

                // Check for terminal tool calls — end the agent run immediately
                let any_terminal = tool_uses.iter().any(|call| {
                    let name = call.get("name").and_then(Value::as_str).unwrap_or("");
                    session_chat.tool_manager().is_terminal(name)
                });
                if any_terminal {
                    let terminal_result = extract_terminal_tool_result(&tool_results);
                    let result = handler
                        .on_end_turn(terminal_result, StopReason::EndTurn, &[], None)
                        .await?;
                    metadata.iterations = iterations;
                    metadata.duration = started_at.elapsed();
                    return Ok((result, metadata));
                }

                history.push(ChatMessage {
                    role: Role::User,
                    content: tool_results,
                });

                handler
                    .post_tool_iteration(history, response.usage.input_tokens)
                    .await?;

                // Check for OPERATOR interrupts (steering messages or /stop)
                if let Some(ref mut rx) = interrupt_rx {
                    match drain_interrupts(rx, history, session_chat.db(), session_id).await? {
                        InterruptAction::Continue => {}
                        InterruptAction::Stop => {
                            metadata.iterations = iterations;
                            metadata.duration = started_at.elapsed();
                            let fallback = last_result.unwrap_or(ChatResult {
                                message: String::new(),
                                stop_reason: ChatStopReason::Stopped,
                            });
                            return Ok((
                                ChatResult {
                                    stop_reason: ChatStopReason::Stopped,
                                    ..fallback
                                },
                                metadata,
                            ));
                        }
                    }
                }
            }
            StopReason::EndTurn | StopReason::MaxTokens => {
                let tool_uses = extract_tool_use_blocks(&response.content);
                let message = extract_text_content(&response.content);

                // Detect empty EndTurn: no text and no tool calls.
                if message.is_empty() && tool_uses.is_empty() {
                    let content_json = serde_json::to_string(&response.content)
                        .unwrap_or_else(|e| format!("<serialization failed: {e}>"));
                    if !retried_empty {
                        tracing::warn!(
                            iterations = iterations as u64,
                            stop_reason = format!("{:?}", response.stop_reason),
                            content = content_json,
                            "empty EndTurn response, injecting recovery nudge",
                        );
                        // Inject a recovery nudge to snap the model out of
                        // the empty-response state. Just retrying with the
                        // same history produces the same result.
                        history.push(ChatMessage {
                            role: Role::System,
                            content: vec![ContentBlock::Text {
                                text: "<system-reminder>You returned an empty \
                                       response. This is a bug — your session \
                                       will end if it happens again. Continue \
                                       by making a tool call now.</system-reminder>"
                                    .to_string(),
                            }],
                        });
                        retried_empty = true;
                        continue;
                    }
                    tracing::error!(
                        iterations = iterations as u64,
                        stop_reason = format!("{:?}", response.stop_reason),
                        content = content_json,
                        "empty EndTurn response after recovery nudge",
                    );
                    return Err(ChatError::Provider(ProviderError::EmptyResponse {
                        detail: "provider returned empty EndTurn twice".to_string(),
                    }));
                }

                // Check progress gate: does the handler want the model
                // to keep working instead of ending? Max 3 retries to
                // break through "commitment loop" patterns where the
                // model says "I'll continue" without making tool calls.
                if progress_gate_retries < 3
                    && let Some(nudge) = handler.check_progress_gate(history).await?
                {
                    tracing::warn!(
                        iterations = iterations as u64,
                        response_len = message.len() as u64,
                        gate_retry = progress_gate_retries as u64,
                        "progress gate triggered — model tried to end \
                         prematurely, injecting continuation nudge",
                    );
                    history.push(ChatMessage {
                        role: Role::System,
                        content: vec![ContentBlock::Text { text: nudge }],
                    });
                    progress_gate_retries += 1;
                    iterations += 1;
                    continue;
                }

                let raw_output = raw_output_to_values(&response.content);
                let result = handler
                    .on_end_turn(message, response.stop_reason, &tool_uses, raw_output)
                    .await?;

                // Drain pending interrupts before exiting. If a user message
                // arrived during the final LLM call, persist it and continue
                // so the model sees and responds to it.
                if let Some(ref mut rx) = interrupt_rx {
                    let pre_drain_len = history.len();
                    match drain_interrupts(rx, history, session_chat.db(), session_id).await? {
                        InterruptAction::Stop => {
                            metadata.iterations = iterations;
                            metadata.duration = started_at.elapsed();
                            return Ok((result, metadata));
                        }
                        InterruptAction::Continue => {
                            if history.len() > pre_drain_len {
                                // New user messages were injected. Push the
                                // assistant's EndTurn response into history
                                // and continue so the model responds to them.
                                history.push(ChatMessage {
                                    role: Role::Assistant,
                                    content: response.content,
                                });
                                last_result = None;
                                iterations += 1;
                                continue;
                            }
                        }
                    }
                }

                tracing::info!(
                    iterations = iterations as u64,
                    stop_reason = format!("{:?}", result.stop_reason),
                    response_len = result.message.len() as u64,
                    response = &result.message,
                    "agent run complete",
                );
                metadata.iterations = iterations;
                metadata.duration = started_at.elapsed();
                return Ok((result, metadata));
            }
        }

        last_result = Some(ChatResult {
            message: extract_latest_assistant_text(history),
            stop_reason: ChatStopReason::EndTurn,
        });
    }
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
        ("shell", "background") => val == &Value::Bool(false),
        ("shell", "timeout_ms") => val.as_u64() == Some(30_000),
        ("shell", "directory") => val
            .as_str()
            .is_some_and(|s| matches!(s, "" | ".") || std::path::Path::new(s) == workspace),
        ("web_fetch", "scroll") => val == &Value::Bool(false),
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
