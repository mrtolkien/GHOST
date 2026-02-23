use async_trait::async_trait;
use serde_json::{Value, json};

use crate::providers::ToolDefinition;

use super::context::ToolContext;
use super::error::ToolError;
use super::manager::Tool;

pub struct AgentControl;

#[async_trait]
impl Tool for AgentControl {
    fn name(&self) -> &str {
        "agent_control"
    }

    fn schema(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Control background agents. Use 'start' to spawn \
                an agent, 'continue' to resume a completed agent with new \
                instructions, 'status' to check progress, or 'stop' to \
                terminate."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["start", "continue", "status", "stop"],
                        "description": "The action to perform"
                    },
                    "agent": {
                        "type": "string",
                        "description": "For 'start': agent name (e.g. 'deep-research')"
                    },
                    "prompt": {
                        "type": "string",
                        "description": "For 'start'/'continue': the prompt or follow-up instructions"
                    },
                    "agent_id": {
                        "type": "string",
                        "description": "For 'continue'/'status'/'stop': the agent_id returned by 'start'"
                    }
                },
                "required": ["action"],
                "additionalProperties": false
            }),
        }
    }

    #[tracing::instrument(skip_all, fields(tool = "agent_control"))]
    async fn execute(&self, params: Value, ctx: &ToolContext) -> Result<String, ToolError> {
        let action = params
            .get("action")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ToolError::InvalidParams("missing required parameter 'action'".to_string())
            })?;

        let runner = ctx
            .task_runner
            .as_ref()
            .ok_or_else(|| ToolError::ExecutionFailed("agent runner not available".to_string()))?;

        match action {
            "start" => self.action_start(&params, ctx, runner).await,
            "continue" => self.action_continue(&params, ctx, runner).await,
            "status" => self.action_status(&params, runner).await,
            "stop" => self.action_stop(&params, runner).await,
            _ => Err(ToolError::InvalidParams(format!(
                "unknown action '{action}': must be one of \
                 start, continue, status, stop"
            ))),
        }
    }
}

impl AgentControl {
    async fn action_start(
        &self,
        params: &Value,
        ctx: &ToolContext,
        runner: &crate::agents::TaskRunner,
    ) -> Result<String, ToolError> {
        let agent_name = params.get("agent").and_then(Value::as_str).ok_or_else(|| {
            ToolError::InvalidParams("'start' requires an 'agent' name".to_string())
        })?;

        let prompt = params
            .get("prompt")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::InvalidParams("'start' requires a 'prompt'".to_string()))?;

        let parent_session_id = parse_session_thing_opt(&ctx.session_id);

        let agent_id = runner
            .start(agent_name, prompt, parent_session_id.as_ref())
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        Ok(format!(
            "Agent '{agent_name}' started (agent_id: {agent_id}). \
             Check progress with agent_control(action: 'status', agent_id: '{agent_id}')."
        ))
    }

    async fn action_continue(
        &self,
        params: &Value,
        ctx: &ToolContext,
        runner: &crate::agents::TaskRunner,
    ) -> Result<String, ToolError> {
        let agent_id = params
            .get("agent_id")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ToolError::InvalidParams("'continue' requires an 'agent_id'".to_string())
            })?;

        let prompt = params
            .get("prompt")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ToolError::InvalidParams("'continue' requires a 'prompt'".to_string())
            })?;

        let parent_session_id = parse_session_thing_opt(&ctx.session_id);

        let agent_name = runner
            .continue_task(agent_id, prompt, parent_session_id.as_ref())
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        Ok(format!(
            "Agent '{agent_name}' continued (agent_id: {agent_id}). \
             Check progress with agent_control(action: 'status', \
             agent_id: '{agent_id}')."
        ))
    }

    async fn action_status(
        &self,
        params: &Value,
        runner: &crate::agents::TaskRunner,
    ) -> Result<String, ToolError> {
        let agent_id = params
            .get("agent_id")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ToolError::InvalidParams("'status' requires an 'agent_id'".to_string())
            })?;

        let status = runner
            .status(agent_id)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        let mut output = format!(
            "Agent '{}' — {}\n\
             Messages: {}\n",
            status.agent_name, status.status, status.message_count,
        );

        if let Some(todo) = &status.todo_summary {
            output.push_str(&format!("\n{todo}"));
        }

        if let Some(findings) = &status.findings {
            output.push_str(&format!("\n## Findings\n{findings}"));
        }

        Ok(output)
    }

    async fn action_stop(
        &self,
        params: &Value,
        runner: &crate::agents::TaskRunner,
    ) -> Result<String, ToolError> {
        let agent_id = params
            .get("agent_id")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::InvalidParams("'stop' requires an 'agent_id'".to_string()))?;

        let status = runner
            .stop(agent_id)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        let mut output = format!(
            "Agent '{}' stopped.\nMessages: {}\n",
            status.agent_name, status.message_count,
        );

        if let Some(findings) = &status.findings {
            output.push_str(&format!("\n## Partial Findings\n{findings}"));
        }

        Ok(output)
    }
}

fn parse_session_thing_opt(session_id: &str) -> Option<surrealdb::sql::Thing> {
    if let Some((table, id)) = session_id.split_once(':') {
        if table.is_empty() || id.is_empty() {
            return None;
        }
        Some(surrealdb::sql::Thing::from((table, id)))
    } else if !session_id.is_empty() {
        Some(surrealdb::sql::Thing::from(("session", session_id)))
    } else {
        None
    }
}
