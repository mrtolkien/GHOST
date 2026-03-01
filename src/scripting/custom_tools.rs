use std::sync::Arc;

use async_trait::async_trait;
use mlua::prelude::*;
use serde_json::Value;

use crate::providers::ToolDefinition;
use crate::tools::context::ToolContext;
use crate::tools::error::ToolError;
use crate::tools::manager::Tool;

use super::host::ScriptHost;

/// Adapter that wraps a Lua-defined tool handler as a Rust `Tool`.
///
/// The handler function lives in the ScriptHost's Lua VM registry. When
/// `execute` is called, we retrieve the function, convert JSON params to
/// a Lua table, call the handler, and convert the result back to a string.
pub struct LuaToolAdapter {
    tool_name: String,
    description: String,
    input_schema: Value,
    terminal: bool,
    handler_key_index: usize,
    script_host: Arc<ScriptHost>,
}

impl std::fmt::Debug for LuaToolAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LuaToolAdapter")
            .field("tool_name", &self.tool_name)
            .field("terminal", &self.terminal)
            .finish()
    }
}

impl LuaToolAdapter {
    pub fn new(
        tool_name: String,
        description: String,
        input_schema: Value,
        terminal: bool,
        handler_key_index: usize,
        script_host: Arc<ScriptHost>,
    ) -> Self {
        Self {
            tool_name,
            description,
            input_schema,
            terminal,
            handler_key_index,
            script_host,
        }
    }
}

#[async_trait]
impl Tool for LuaToolAdapter {
    fn name(&self) -> &str {
        &self.tool_name
    }

    fn schema(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.tool_name.clone(),
            description: self.description.clone(),
            input_schema: self.input_schema.clone(),
        }
    }

    fn is_terminal(&self) -> bool {
        self.terminal
    }

    async fn execute(&self, params: Value, _ctx: &ToolContext) -> Result<String, ToolError> {
        let lua = self.script_host.lua();
        let key = self
            .script_host
            .tool_handler_key(self.handler_key_index)
            .ok_or_else(|| {
                ToolError::ExecutionFailed(format!(
                    "lua tool '{}': handler registry key not found",
                    self.tool_name
                ))
            })?;

        let handler: LuaFunction = lua.registry_value(key).map_err(|e| {
            ToolError::ExecutionFailed(format!(
                "lua tool '{}': failed to retrieve handler: {e}",
                self.tool_name
            ))
        })?;

        // Convert JSON params to Lua table
        let lua_args = json_to_lua(lua, &params).map_err(|e| {
            ToolError::ExecutionFailed(format!(
                "lua tool '{}': failed to convert params: {e}",
                self.tool_name
            ))
        })?;

        // Pass ctx (registered during build hook) as first arg
        let ctx_val: LuaValue = lua.globals().get("ctx").unwrap_or(LuaValue::Nil);
        let result: LuaValue = handler.call((ctx_val, lua_args)).map_err(|e| {
            ToolError::ExecutionFailed(format!("lua tool '{}': handler error: {e}", self.tool_name))
        })?;

        match result {
            LuaValue::String(s) => s
                .to_str()
                .map(|s| s.to_string())
                .map_err(|e| ToolError::ExecutionFailed(format!("invalid UTF-8 in result: {e}"))),
            LuaValue::Nil => Ok(String::new()),
            other => Ok(format!("{other:?}")),
        }
    }
}

/// Convert a serde_json Value to a Lua value.
fn json_to_lua(lua: &Lua, value: &Value) -> LuaResult<LuaValue> {
    match value {
        Value::Null => Ok(LuaValue::Nil),
        Value::Bool(b) => Ok(LuaValue::Boolean(*b)),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(LuaValue::Integer(i))
            } else {
                Ok(LuaValue::Number(n.as_f64().unwrap_or(0.0)))
            }
        }
        Value::String(s) => Ok(LuaValue::String(lua.create_string(s)?)),
        Value::Array(arr) => {
            let table = lua.create_table()?;
            for (i, v) in arr.iter().enumerate() {
                table.raw_set(i + 1, json_to_lua(lua, v)?)?;
            }
            Ok(LuaValue::Table(table))
        }
        Value::Object(map) => {
            let table = lua.create_table()?;
            for (k, v) in map {
                table.set(k.as_str(), json_to_lua(lua, v)?)?;
            }
            Ok(LuaValue::Table(table))
        }
    }
}

/// Build `LuaToolAdapter` instances from an agent's custom tool definitions.
pub fn build_custom_tools(
    config: &super::types::AgentConfig,
    script_host: &Arc<ScriptHost>,
) -> Vec<Arc<dyn Tool>> {
    config
        .custom_tools
        .iter()
        .map(|def| {
            Arc::new(LuaToolAdapter::new(
                def.name.clone(),
                def.description.clone(),
                def.parameters.clone(),
                def.terminal,
                def.handler_key_index,
                Arc::clone(script_host),
            )) as Arc<dyn Tool>
        })
        .collect()
}
