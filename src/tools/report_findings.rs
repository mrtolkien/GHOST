use async_trait::async_trait;
use serde_json::{Value, json};

use crate::providers::ToolDefinition;

use super::context::ToolContext;
use super::error::ToolError;
use super::manager::Tool;

/// The name of the report_findings tool, used by the tool loop to intercept
/// it as a terminal tool (same mechanism as `respond`).
pub const REPORT_FINDINGS_TOOL_NAME: &str = "report_findings";

pub struct ReportFindings;

#[async_trait]
impl Tool for ReportFindings {
    fn name(&self) -> &str {
        REPORT_FINDINGS_TOOL_NAME
    }

    fn schema(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Submit your COMPLETE research findings. REQUIREMENTS: \
                (1) You must have called web_fetch on at least 5 different URLs \
                before calling this — if your citations array has fewer than 5 \
                entries, STOP and go read more pages first. \
                (2) At least 2 citations must be from domain-specialist sites or \
                individual expert reviewers, not generalist publications. \
                (3) All sub-questions must be answered with evidence from pages \
                you actually read. Do not output findings as plain text."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "message": {
                        "type": "string",
                        "description": "Your complete research findings in markdown"
                    },
                    "citations": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "source": {
                                    "type": "string",
                                    "description": "URL of a page you web_fetch'd"
                                },
                                "context": {
                                    "type": "string",
                                    "description": "What this source contributed to your findings"
                                }
                            },
                            "required": ["source", "context"],
                            "additionalProperties": false
                        },
                        "description": "Every source you actually read (web_fetch'd). \
                            Do NOT cite pages you only saw in search snippets."
                    }
                },
                "required": ["message", "citations"],
                "additionalProperties": false
            }),
        }
    }

    #[tracing::instrument(skip_all, fields(tool = "report_findings"))]
    async fn execute(&self, _params: Value, _ctx: &ToolContext) -> Result<String, ToolError> {
        // Intercepted by the tool loop before execution, like `respond`.
        Err(ToolError::ExecutionFailed(
            "report_findings tool should be intercepted by the tool loop".to_string(),
        ))
    }
}
