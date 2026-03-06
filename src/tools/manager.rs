use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use crate::providers::ToolDefinition;

use super::context::ToolContext;
use super::error::ToolError;

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn schema(&self) -> ToolDefinition;
    async fn execute(&self, params: Value, ctx: &ToolContext) -> Result<String, ToolError>;

    /// Whether calling this tool should end the agent's run immediately.
    /// Used by Lua-defined custom tools with `terminal = true`.
    fn is_terminal(&self) -> bool {
        false
    }
}

#[derive(Default)]
pub struct ToolManager {
    tools: HashMap<String, Arc<dyn Tool>>,
}

impl std::fmt::Debug for ToolManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolManager")
            .field("tool_count", &self.tools.len())
            .field("tools", &self.tools.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl ToolManager {
    /// Create an empty `ToolManager` with no tools registered. Useful for
    /// tests that mock provider responses without real tool execution.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    /// Create a `ToolManager` with the standard chat tool set.
    #[must_use]
    pub fn for_chat() -> Self {
        let mut manager = Self::default();
        manager.register(Arc::new(super::shell::RunShellCommand));
        manager.register(Arc::new(super::read_file::ReadFile));
        manager.register(Arc::new(super::write_file::WriteFile));
        manager.register(Arc::new(super::file_edit::FileEdit));
        manager.register(Arc::new(super::todo::Todo));
        manager.register(Arc::new(super::knowledge_search::KnowledgeSearch));
        manager.register(Arc::new(super::web_search::WebSearch));
        manager.register(Arc::new(super::web_fetch::WebFetch));
        manager.register(Arc::new(super::agent_control::AgentControl));
        manager
    }

    /// Create a `ToolManager` for an agent, restricted to a whitelist of
    /// tool names. Unknown names are silently ignored.
    #[must_use]
    pub fn for_agent(allowed: &[String]) -> Self {
        let full = Self::all_available();
        let mut manager = Self::default();
        for name in allowed {
            if let Some(tool) = full.tools.get(name.as_str()) {
                manager.tools.insert(name.clone(), Arc::clone(tool));
            }
        }
        manager
    }

    /// Build a registry containing every tool the system knows about.
    fn all_available() -> Self {
        let mut manager = Self::default();
        manager.register(Arc::new(super::shell::RunShellCommand));
        manager.register(Arc::new(super::read_file::ReadFile));
        manager.register(Arc::new(super::write_file::WriteFile));
        manager.register(Arc::new(super::file_edit::FileEdit));
        manager.register(Arc::new(super::todo::Todo));
        manager.register(Arc::new(super::knowledge_search::KnowledgeSearch));
        manager.register(Arc::new(super::web_search::WebSearch));
        manager.register(Arc::new(super::web_fetch::WebFetch));
        manager.register(Arc::new(super::note_write::NoteWrite));
        manager.register(Arc::new(super::agent_control::AgentControl));
        manager
    }

    #[must_use]
    pub fn all_tool_schemas(&self) -> Vec<ToolDefinition> {
        self.tools.values().map(|tool| tool.schema()).collect()
    }

    /// Check whether a tool is marked as terminal (ends the agent run).
    #[must_use]
    pub fn is_terminal(&self, tool_name: &str) -> bool {
        self.tools
            .get(tool_name)
            .is_some_and(|tool| tool.is_terminal())
    }

    #[tracing::instrument(name = "run tool", skip_all, fields(
        gen_ai.tool.name=%tool_name,
        gen_ai.tool.call.arguments=%params
    ))]
    pub async fn execute(
        &self,
        tool_name: &str,
        params: Value,
        ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        let tool = self
            .tools
            .get(tool_name)
            .ok_or_else(|| ToolError::NotFound {
                name: tool_name.to_string(),
            })?;

        let result = tool.execute(params, ctx).await;

        match &result {
            Ok(output) => {
                let truncated: String = output.chars().take(2000).collect();
                logfire::info!(
                    "tool executed",
                    output_len = output.len() as u64,
                    output = truncated,
                );
            }
            Err(err) => {
                logfire::warn!("tool execution failed", error = err.to_string(),);
            }
        }

        result
    }
}
