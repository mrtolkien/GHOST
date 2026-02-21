use async_trait::async_trait;
use serde_json::Value;

use crate::providers::types::{DebugContext, ProviderError};
use crate::providers::{ChatMessage, ChatRequest, ContentBlock, Role, StopReason};

use super::convert::{
    extract_latest_assistant_text, extract_text_content, extract_tool_use_blocks,
    raw_output_to_values,
};
use super::session::SessionChat;
use super::types::{ChatError, ChatResult, ChatStopReason};

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
    ) -> Result<(), ChatError> {
        Ok(())
    }
}

/// Shared tool-use loop for both interactive chat and background jobs.
#[tracing::instrument(skip_all, level = "debug", fields(session_id = session_id))]
pub(super) async fn run_tool_loop(
    session_chat: &SessionChat,
    session_id: &str,
    model: &str,
    max_iterations: usize,
    handler: &mut (impl ToolLoopHandler + ?Sized),
    history: &mut Vec<ChatMessage>,
) -> Result<ChatResult, ChatError> {
    let mut iterations = 0usize;
    let mut last_result: Option<ChatResult> = None;
    let mut retried_empty = false;

    loop {
        let prompt = handler.system_prompt()?;
        let request = ChatRequest {
            model: model.to_string(),
            messages: history.clone(),
            tools: Some(session_chat.tool_manager().all_tool_schemas()),
            max_tokens: None,
            temperature: None,
            system: Some(prompt),
            debug_context: Some(DebugContext {
                session_id: session_id.to_string(),
                iteration: iterations,
            }),
        };
        let response = session_chat
            .provider()
            .chat(request)
            .await
            .map_err(ChatError::from)?;

        match response.stop_reason {
            StopReason::ToolUse => {
                if iterations >= max_iterations {
                    let fallback = last_result.unwrap_or(ChatResult {
                        message: "Hit tool iteration limit before completing response.".to_string(),
                        stop_reason: ChatStopReason::MaxIterations,
                    });
                    return Ok(ChatResult {
                        stop_reason: ChatStopReason::MaxIterations,
                        ..fallback
                    });
                }
                iterations += 1;

                let tool_uses = extract_tool_use_blocks(&response.content);
                let raw_output = raw_output_to_values(&response.content);

                let assistant_text = extract_text_content(&response.content);
                handler
                    .on_assistant_tool_use(&assistant_text, &tool_uses, raw_output)
                    .await?;

                history.push(ChatMessage {
                    role: Role::Assistant,
                    content: response.content,
                });

                let tool_results = session_chat
                    .execute_tool_calls(session_id, &tool_uses)
                    .await;
                handler.on_tool_results(&tool_results).await?;

                history.push(ChatMessage {
                    role: Role::User,
                    content: tool_results,
                });

                handler.post_tool_iteration(history).await?;
            }
            StopReason::EndTurn | StopReason::MaxTokens => {
                let tool_uses = extract_tool_use_blocks(&response.content);
                let message = extract_text_content(&response.content);

                // Detect empty EndTurn: no text and no tool calls.
                if message.is_empty() && tool_uses.is_empty() {
                    let content_json = serde_json::to_string(&response.content)
                        .unwrap_or_else(|e| format!("<serialization failed: {e}>"));
                    if !retried_empty {
                        logfire::warn!(
                            "empty EndTurn response, retrying",
                            iterations = iterations as u64,
                            stop_reason = format!("{:?}", response.stop_reason),
                            content = content_json,
                        );
                        retried_empty = true;
                        continue;
                    }
                    logfire::error!(
                        "empty EndTurn response after retry",
                        iterations = iterations as u64,
                        stop_reason = format!("{:?}", response.stop_reason),
                        content = content_json,
                    );
                    return Err(ChatError::Provider(ProviderError::EmptyResponse {
                        detail: "provider returned empty EndTurn twice".to_string(),
                    }));
                }

                let raw_output = raw_output_to_values(&response.content);
                let result = handler
                    .on_end_turn(message, response.stop_reason, &tool_uses, raw_output)
                    .await?;
                logfire::info!(
                    "tool loop complete",
                    iterations = iterations as u64,
                    stop_reason = format!("{:?}", result.stop_reason),
                    response_len = result.message.len() as u64,
                );
                return Ok(result);
            }
        }

        last_result = Some(ChatResult {
            message: extract_latest_assistant_text(history),
            stop_reason: ChatStopReason::EndTurn,
        });
    }
}
