//! TEMPORARY SCAFFOLDING
//! This module exists to unblock spec 06 chat orchestration before spec 10 is fully
//! implemented. It is intentionally minimal and may be rewritten entirely when the
//! real tool system lands.
//! WARNING: This file currently contains implementation code. Move logic into
//! dedicated module files before real tools feature development continues.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use surrealdb::Surreal;
use surrealdb::engine::local::Db;

use crate::config::Config;
use crate::providers::ToolDefinition;

#[derive(Debug, Clone)]
pub enum ToolSet {
    Chat,
    Job,
}

#[derive(Debug, Clone)]
pub struct ToolContext {
    pub workspace: PathBuf,
    pub cwd: PathBuf,
    pub db: Surreal<Db>,
    pub config: Config,
    pub session_id: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("tool '{name}' not found")]
    NotFound { name: String },

    #[error("tool '{name}' failed: {message}")]
    ExecutionFailed { name: String, message: String },
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
            .finish()
    }
}

impl ToolManager {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    #[must_use]
    pub fn all_tool_schemas(&self, _tool_set: ToolSet) -> Vec<ToolDefinition> {
        self.tools.values().map(|tool| tool.schema()).collect()
    }

    #[tracing::instrument(skip_all, fields(tool_name = tool_name))]
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
        tool.execute(params, ctx).await
    }
}
