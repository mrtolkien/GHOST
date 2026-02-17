use async_trait::async_trait;
use serde_json::{Value, json};

use crate::providers::ToolDefinition;

use super::context::ToolContext;
use super::error::ToolError;
use super::manager::Tool;

/// The name of the respond tool, used by the chat loop to intercept it.
pub const RESPOND_TOOL_NAME: &str = "respond";

pub struct Respond;

#[async_trait]
impl Tool for Respond {
    fn name(&self) -> &str {
        RESPOND_TOOL_NAME
    }

    fn schema(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Send your final response to the OPERATOR. You MUST call \
                this tool to deliver every answer — do not respond with plain \
                text outside of a tool call.\n\n\
                Why this tool exists: the system needs structured output \
                (message + citations) alongside tool calling, but a JSON \
                response_format conflicts with tool use on most models. By \
                making the response itself a tool, both regular tools and \
                structured output work through the same mechanism."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "message": {
                        "type": "string",
                        "description": "Your response to the OPERATOR"
                    },
                    "citations": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "source": {
                                    "type": "string",
                                    "description": "File path or URL used"
                                },
                                "context": {
                                    "type": "string",
                                    "description": "What this source was used for"
                                }
                            },
                            "required": ["source", "context"],
                            "additionalProperties": false
                        },
                        "description": "Sources referenced in the response (empty array if none)"
                    }
                },
                "required": ["message", "citations"],
                "additionalProperties": false
            }),
        }
    }

    #[tracing::instrument(skip_all, fields(tool = "respond"))]
    async fn execute(&self, _params: Value, _ctx: &ToolContext) -> Result<String, ToolError> {
        // This tool is intercepted by the chat loop before execution.
        // If we reach here, something went wrong in the routing.
        Err(ToolError::ExecutionFailed(
            "respond tool should be intercepted by the chat loop".to_string(),
        ))
    }
}
