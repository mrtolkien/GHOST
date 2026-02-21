use async_trait::async_trait;
use serde_json::Value;

use crate::providers::types::DebugContext;
use crate::providers::{ChatMessage, ChatRequest, ContentBlock, Role, StopReason};
use crate::tools::{REPORT_FINDINGS_TOOL_NAME, RESPOND_TOOL_NAME};

use super::convert::{
    extract_latest_assistant_text, extract_text_content, extract_tool_use_blocks,
    parse_respond_call, raw_output_to_values,
};
use super::session::SessionChat;
use super::types::{ChatError, ChatResult, ChatStopReason, Citation};

/// Minimum number of `web_fetch` calls required before `report_findings` is
/// accepted. If the agent tries to report with fewer fetches, the report is
/// rejected and the agent is told to read more pages.
const MIN_REPORT_FETCHES: usize = 5;

/// Handler for tool loop events.
///
/// Implementations customize how the loop records assistant messages, tool
/// results, and final answers — e.g. persisting to DB (interactive chat)
/// vs appending to a transcript (background jobs).
#[async_trait]
pub(super) trait ToolLoopHandler: Send {
    fn system_prompt(&self) -> Result<String, ChatError>;

    async fn on_respond(
        &mut self,
        message: String,
        citations: Vec<Citation>,
        tool_uses: &[Value],
        raw_output: Option<Vec<Value>>,
    ) -> Result<ChatResult, ChatError>;

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
                let raw_output = raw_output_to_values(&response.content);

                // Check for `respond` first (always accepted)
                if let Some((message, mut citations)) =
                    parse_respond_call(RESPOND_TOOL_NAME, &tool_uses)
                {
                    session_chat.resolve_citation_urls(&mut citations);
                    let result = handler
                        .on_respond(message, citations, &tool_uses, raw_output.clone())
                        .await?;
                    logfire::info!(
                        "tool loop complete (respond)",
                        iterations = iterations as u64,
                        citation_count = result.citations.len() as u64,
                        response_len = result.message.len() as u64,
                    );
                    return Ok(result);
                }

                // Check for `report_findings` — enforce minimum web_fetch count
                if let Some((message, mut citations)) =
                    parse_respond_call(REPORT_FINDINGS_TOOL_NAME, &tool_uses)
                {
                    let fetch_count = count_web_fetches(history);
                    if fetch_count < MIN_REPORT_FETCHES {
                        // Reject the report and tell the agent to read more
                        logfire::warn!(
                            "report_findings rejected: insufficient web_fetch calls",
                            fetch_count = fetch_count,
                            min_required = MIN_REPORT_FETCHES,
                        );
                        history.push(ChatMessage {
                            role: Role::Assistant,
                            content: response.content,
                        });
                        history.push(ChatMessage {
                            role: Role::User,
                            content: vec![ContentBlock::Text {
                                text: format!(
                                    "REJECTED: You have only called web_fetch {fetch_count} \
                                     times. You must read at least {MIN_REPORT_FETCHES} \
                                     different pages before reporting. Go back to Step 3 and \
                                     read more pages from the specialist sources you identified."
                                ),
                            }],
                        });
                        continue;
                    }
                    session_chat.resolve_citation_urls(&mut citations);
                    let result = handler
                        .on_respond(message, citations, &tool_uses, raw_output.clone())
                        .await?;
                    logfire::info!(
                        "tool loop complete (report_findings)",
                        iterations = iterations,
                        citation_count = result.citations.len(),
                        response_len = result.message.len(),
                        web_fetches = fetch_count,
                    );
                    return Ok(result);
                }

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
                let raw_output = raw_output_to_values(&response.content);
                let message = extract_text_content(&response.content);
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
            citations: Vec::new(),
            stop_reason: ChatStopReason::EndTurn,
        });
    }
}

/// Count how many `web_fetch` tool calls appear in the conversation history.
fn count_web_fetches(history: &[ChatMessage]) -> usize {
    history
        .iter()
        .flat_map(|msg| msg.content.iter())
        .filter(|block| matches!(block, ContentBlock::ToolUse { name, .. } if name == "web_fetch"))
        .count()
}
