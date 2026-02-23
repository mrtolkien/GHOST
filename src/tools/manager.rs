use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use crate::providers::ToolDefinition;

use super::context::ToolContext;
use super::error::ToolError;

#[derive(Debug, Clone)]
pub enum ToolSet {
    Chat,
    Reflection,
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn schema(&self) -> ToolDefinition;
    async fn execute(&self, params: Value, ctx: &ToolContext) -> Result<String, ToolError>;
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

    /// Create a `ToolManager` for reflection jobs.
    ///
    /// Explicit tool list: all chat tools except `agent_control`, plus
    /// knowledge-write tools (`note_write`, `reference_manage`).
    #[must_use]
    pub fn for_reflection() -> Self {
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
        manager.register(Arc::new(super::reference_manage::ReferenceManage));
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
        let mut manager = Self::for_reflection();
        manager.register(Arc::new(super::agent_control::AgentControl));
        manager
    }

    #[must_use]
    pub fn all_tool_schemas(&self) -> Vec<ToolDefinition> {
        self.tools.values().map(|tool| tool.schema()).collect()
    }

    #[tracing::instrument(skip_all, fields(tool_name))]
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

        let start = std::time::Instant::now();
        let result = tool.execute(params, ctx).await;
        let elapsed = start.elapsed();
        let tool_name_owned = tool_name.to_string();

        match &result {
            Ok(output) => {
                logfire::info!(
                    "tool executed",
                    tool = tool_name_owned,
                    elapsed_ms = elapsed.as_millis() as u64,
                    output_len = output.len() as u64,
                );
            }
            Err(err) => {
                logfire::warn!(
                    "tool execution failed",
                    tool = tool_name_owned,
                    elapsed_ms = elapsed.as_millis() as u64,
                    error = err.to_string(),
                );
            }
        }

        result
    }
}
