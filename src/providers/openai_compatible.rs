use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::providers::types::{
    ChatRequest, ChatResponse, ContentBlock, ProviderError, ResponseFormat, Role, StopReason,
    ToolDefinition, Usage,
};

#[derive(Debug, Serialize)]
pub(crate) struct ChatCompletionsRequest {
    pub(crate) model: String,
    pub(crate) messages: Vec<OpenAiMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) tools: Option<Vec<OpenAiToolDefinition>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) response_format: Option<OpenAiResponseFormat>,
}

#[derive(Debug, Serialize)]
pub(crate) struct OpenAiResponseFormat {
    pub(crate) r#type: String,
    pub(crate) json_schema: OpenAiJsonSchemaFormat,
}

#[derive(Debug, Serialize)]
pub(crate) struct OpenAiJsonSchemaFormat {
    pub(crate) name: String,
    pub(crate) schema: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct OpenAiMessage {
    pub(crate) role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) content: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) tool_call_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ToolCall {
    pub(crate) id: String,
    pub(crate) r#type: String,
    pub(crate) function: ToolCallFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ToolCallFunction {
    pub(crate) name: String,
    pub(crate) arguments: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct OpenAiToolDefinition {
    pub(crate) r#type: String,
    pub(crate) function: OpenAiFunctionDefinition,
}

#[derive(Debug, Serialize)]
pub(crate) struct OpenAiFunctionDefinition {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) parameters: Value,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ChatCompletionsResponse {
    pub(crate) model: Option<String>,
    pub(crate) choices: Vec<Choice>,
    #[serde(default)]
    pub(crate) usage: Option<UsageResponse>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Choice {
    pub(crate) message: OpenAiMessage,
    #[serde(default)]
    pub(crate) finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct UsageResponse {
    #[serde(default)]
    pub(crate) prompt_tokens: u32,
    #[serde(default)]
    pub(crate) completion_tokens: u32,
    #[serde(default)]
    pub(crate) prompt_tokens_details: Option<TokenDetails>,
    #[serde(default)]
    pub(crate) input_tokens_details: Option<TokenDetails>,
    #[serde(default)]
    pub(crate) cache_read_input_tokens: Option<u32>,
    #[serde(default)]
    pub(crate) cache_creation_input_tokens: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct TokenDetails {
    #[serde(default)]
    pub(crate) cached_tokens: Option<u32>,
    #[serde(default)]
    pub(crate) cache_read: Option<u32>,
    #[serde(default)]
    pub(crate) cache_creation: Option<u32>,
    #[serde(default)]
    pub(crate) cache_read_input_tokens: Option<u32>,
    #[serde(default)]
    pub(crate) cache_creation_input_tokens: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ProviderErrorBody {
    pub(crate) error: Option<ProviderErrorPayload>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ProviderErrorPayload {
    pub(crate) message: Option<String>,
}

pub(crate) fn build_request_body(request: &ChatRequest) -> ChatCompletionsRequest {
    ChatCompletionsRequest {
        model: request.model.clone(),
        messages: convert_messages(request),
        tools: request
            .tools
            .as_ref()
            .map(|tools| tools.iter().map(convert_tool).collect()),
        max_tokens: request.max_tokens,
        temperature: request.temperature,
        response_format: request
            .response_format
            .as_ref()
            .map(convert_response_format),
    }
}

fn convert_response_format(response_format: &ResponseFormat) -> OpenAiResponseFormat {
    match response_format {
        ResponseFormat::JsonSchema { name, schema } => OpenAiResponseFormat {
            r#type: "json_schema".to_string(),
            json_schema: OpenAiJsonSchemaFormat {
                name: name.clone(),
                schema: schema.clone(),
            },
        },
    }
}

pub(crate) fn convert_messages(request: &ChatRequest) -> Vec<OpenAiMessage> {
    let mut messages = Vec::new();

    if let Some(system) = &request.system {
        messages.push(OpenAiMessage {
            role: "system".to_string(),
            content: Some(Value::String(system.clone())),
            tool_calls: None,
            tool_call_id: None,
        });
    }

    for message in &request.messages {
        let mut text_parts = Vec::new();
        let mut tool_calls = Vec::new();
        let mut tool_results = Vec::new();

        for block in &message.content {
            match block {
                ContentBlock::Text { text } => text_parts.push(text.clone()),
                ContentBlock::ToolUse { id, name, input } => {
                    let arguments =
                        serde_json::to_string(input).unwrap_or_else(|_| "{}".to_string());
                    tool_calls.push(ToolCall {
                        id: id.clone(),
                        r#type: "function".to_string(),
                        function: ToolCallFunction {
                            name: name.clone(),
                            arguments,
                        },
                    });
                }
                ContentBlock::ToolResult {
                    tool_use_id,
                    content,
                    ..
                } => {
                    tool_results.push(OpenAiMessage {
                        role: "tool".to_string(),
                        content: Some(Value::String(content.clone())),
                        tool_calls: None,
                        tool_call_id: Some(tool_use_id.clone()),
                    });
                }
            }
        }

        let role = match message.role {
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::System => "system",
        };

        let content = (!text_parts.is_empty()).then(|| Value::String(text_parts.join("\n\n")));
        if content.is_some() || !tool_calls.is_empty() {
            messages.push(OpenAiMessage {
                role: role.to_string(),
                content,
                tool_calls: (!tool_calls.is_empty()).then_some(tool_calls),
                tool_call_id: None,
            });
        }
        messages.extend(tool_results);
    }

    messages
}

fn convert_tool(tool: &ToolDefinition) -> OpenAiToolDefinition {
    OpenAiToolDefinition {
        r#type: "function".to_string(),
        function: OpenAiFunctionDefinition {
            name: tool.name.clone(),
            description: tool.description.clone(),
            parameters: tool.input_schema.clone(),
        },
    }
}

pub(crate) fn parse_response(
    response: ChatCompletionsResponse,
) -> Result<ChatResponse, ProviderError> {
    let choice = response
        .choices
        .into_iter()
        .next()
        .ok_or(ProviderError::EmptyResponse)?;

    let mut content = Vec::new();
    if let Some(content_value) = choice.message.content {
        let text = extract_text_content(content_value);
        if !text.trim().is_empty() {
            content.push(ContentBlock::Text { text });
        }
    }

    if let Some(calls) = choice.message.tool_calls {
        for call in calls {
            let input = serde_json::from_str(&call.function.arguments).map_err(|error| {
                ProviderError::InvalidResponse(format!(
                    "tool call arguments are not valid json: {error}"
                ))
            })?;
            content.push(ContentBlock::ToolUse {
                id: call.id,
                name: call.function.name,
                input,
            });
        }
    }

    if content.is_empty() {
        return Err(ProviderError::EmptyResponse);
    }

    let usage = response.usage.unwrap_or(UsageResponse {
        prompt_tokens: 0,
        completion_tokens: 0,
        prompt_tokens_details: None,
        input_tokens_details: None,
        cache_read_input_tokens: None,
        cache_creation_input_tokens: None,
    });

    let cache_read_tokens = usage
        .cache_read_input_tokens
        .or_else(|| {
            usage
                .prompt_tokens_details
                .as_ref()
                .and_then(|d| d.cache_read_input_tokens)
        })
        .or_else(|| {
            usage
                .input_tokens_details
                .as_ref()
                .and_then(|d| d.cache_read)
        })
        .or_else(|| {
            usage
                .input_tokens_details
                .as_ref()
                .and_then(|d| d.cached_tokens)
        });
    let cache_creation_tokens = usage
        .cache_creation_input_tokens
        .or_else(|| {
            usage
                .prompt_tokens_details
                .as_ref()
                .and_then(|d| d.cache_creation_input_tokens)
        })
        .or_else(|| {
            usage
                .input_tokens_details
                .as_ref()
                .and_then(|d| d.cache_creation)
        });

    Ok(ChatResponse {
        content,
        usage: Usage {
            input_tokens: usage.prompt_tokens,
            output_tokens: usage.completion_tokens,
            cache_read_tokens,
            cache_creation_tokens,
        },
        stop_reason: match choice.finish_reason.as_deref() {
            Some("tool_calls") => StopReason::ToolUse,
            Some("length") => StopReason::MaxTokens,
            _ => StopReason::EndTurn,
        },
        model: response.model.unwrap_or_default(),
    })
}

fn extract_text_content(content: Value) -> String {
    match content {
        Value::String(text) => text,
        Value::Array(parts) => {
            let mut text_parts = Vec::new();
            for part in parts {
                collect_text_fragments(&part, &mut text_parts);
            }
            text_parts.join("\n")
        }
        other => {
            let mut text_parts = Vec::new();
            collect_text_fragments(&other, &mut text_parts);
            text_parts.join("\n")
        }
    }
}

fn collect_text_fragments(value: &Value, output: &mut Vec<String>) {
    match value {
        Value::String(text) => {
            if !text.trim().is_empty() {
                output.push(text.clone());
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_text_fragments(item, output);
            }
        }
        Value::Object(map) => {
            for key in ["text", "content", "output_text"] {
                if let Some(fragment) = map.get(key) {
                    collect_text_fragments(fragment, output);
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::providers::types::{ChatMessage, ContentBlock, ResponseFormat, Role, user_message};

    #[test]
    fn convert_messages_serializes_tool_use_and_result() {
        let request = ChatRequest {
            model: "moonshotai/kimi-k2.5".to_string(),
            messages: vec![
                user_message("hello"),
                ChatMessage {
                    role: Role::Assistant,
                    content: vec![ContentBlock::ToolUse {
                        id: "call_1".to_string(),
                        name: "run_shell_command".to_string(),
                        input: json!({"command": "pwd"}),
                    }],
                },
                ChatMessage {
                    role: Role::User,
                    content: vec![ContentBlock::ToolResult {
                        tool_use_id: "call_1".to_string(),
                        content: "/tmp".to_string(),
                        is_error: false,
                    }],
                },
            ],
            tools: None,
            max_tokens: None,
            temperature: None,
            system: Some("be concise".to_string()),
            response_format: None,
        };

        let messages = convert_messages(&request);
        assert_eq!(messages.len(), 4);
        assert_eq!(messages[0].role, "system");
        assert_eq!(messages[1].role, "user");
        assert_eq!(messages[2].role, "assistant");
        assert!(messages[2].tool_calls.is_some());
        assert_eq!(messages[3].role, "tool");
        assert_eq!(messages[3].tool_call_id.as_deref(), Some("call_1"));
    }

    #[test]
    fn parse_response_handles_text_and_tool_calls() {
        let response = ChatCompletionsResponse {
            model: Some("moonshotai/kimi-k2.5".to_string()),
            choices: vec![Choice {
                message: OpenAiMessage {
                    role: "assistant".to_string(),
                    content: Some(Value::String("ok".to_string())),
                    tool_calls: Some(vec![ToolCall {
                        id: "call_1".to_string(),
                        r#type: "function".to_string(),
                        function: ToolCallFunction {
                            name: "run_shell_command".to_string(),
                            arguments: "{\"command\":\"pwd\"}".to_string(),
                        },
                    }]),
                    tool_call_id: None,
                },
                finish_reason: Some("tool_calls".to_string()),
            }],
            usage: Some(UsageResponse {
                prompt_tokens: 10,
                completion_tokens: 4,
                prompt_tokens_details: None,
                input_tokens_details: None,
                cache_read_input_tokens: Some(1),
                cache_creation_input_tokens: Some(2),
            }),
        };

        let parsed = parse_response(response).expect("parse response");
        assert_eq!(parsed.stop_reason, StopReason::ToolUse);
        assert_eq!(parsed.usage.input_tokens, 10);
        assert_eq!(parsed.usage.output_tokens, 4);
        assert_eq!(parsed.usage.cache_read_tokens, Some(1));
        assert_eq!(parsed.usage.cache_creation_tokens, Some(2));
        assert!(matches!(parsed.content[0], ContentBlock::Text { .. }));
        assert!(matches!(parsed.content[1], ContentBlock::ToolUse { .. }));
    }

    #[test]
    fn extract_text_content_reads_openrouter_output_text_parts() {
        let content = json!([
            {"type":"reasoning","text":"thinking"},
            {"type":"output_text","text":"final answer"}
        ]);
        let text = extract_text_content(content);
        assert!(text.contains("final answer"));
    }

    #[test]
    fn build_request_body_includes_response_format() {
        let request = ChatRequest {
            model: "moonshotai/kimi-k2.5".to_string(),
            messages: vec![user_message("Return structured json.")],
            tools: None,
            max_tokens: None,
            temperature: None,
            system: None,
            response_format: Some(ResponseFormat::JsonSchema {
                name: "short_answer".to_string(),
                schema: json!({
                    "type": "object",
                    "properties": { "answer": { "type": "string" } },
                    "required": ["answer"],
                    "additionalProperties": false
                }),
            }),
        };

        let body = build_request_body(&request);
        let as_json = serde_json::to_value(body).expect("serialize request");
        assert_eq!(as_json["response_format"]["type"], "json_schema");
        assert_eq!(
            as_json["response_format"]["json_schema"]["name"],
            "short_answer"
        );
        assert_eq!(
            as_json["response_format"]["json_schema"]["schema"]["type"],
            "object"
        );
    }
}
