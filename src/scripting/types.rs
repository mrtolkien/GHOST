use std::collections::HashMap;

use mlua::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::providers::types::ReasoningEffort;

/// A custom tool defined in Lua.
#[derive(Debug, Clone)]
pub struct LuaToolDef {
    pub name: String,
    pub description: String,
    pub parameters: Value,
    pub terminal: bool,
    /// Registry key for the Lua handler function. Stored as usize
    /// so the struct stays Send+Sync — the actual key lives in ScriptHost.
    pub handler_key_index: usize,
}

/// Fully parsed agent configuration extracted from `agent.lua`.
#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub name: String,
    pub description: String,
    pub model: Option<String>,
    pub reasoning_effort: Option<ReasoningEffort>,
    pub max_iterations: usize,
    pub tools: Vec<String>,
    pub custom_tools: Vec<LuaToolDef>,
    pub skills: Vec<String>,
    // Hook presence flags (functions live in the Lua VM)
    pub has_build: bool,
    pub has_pre_turn: bool,
    pub has_on_end_turn: bool,
    pub has_post_completion: bool,
    pub has_should_trigger: bool,
    pub has_on_resume: bool,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            description: String::new(),
            model: None,
            reasoning_effort: None,
            max_iterations: 50,
            tools: Vec::new(),
            custom_tools: Vec::new(),
            skills: Vec::new(),
            has_build: false,
            has_pre_turn: false,
            has_on_end_turn: false,
            has_post_completion: false,
            has_should_trigger: false,
            has_on_resume: false,
        }
    }
}

/// Runtime state passed to `pre_turn` and `on_end_turn` hooks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreTurnState {
    pub iteration: usize,
    pub max_iterations: usize,
    pub remaining: usize,
    pub elapsed_seconds: u64,
    pub tool_counts: HashMap<String, u32>,
    pub last_input_tokens: u32,
    pub context_window: usize,
    pub todo_summary: Option<TodoSummary>,
    /// Pre-formatted TODO list text for Lua nudges to inject.
    pub todo_text: Option<String>,
    pub temporal_fire_count: usize,
    pub context_pressure_fired: bool,
}

/// Result of calling the `build(ctx, args)` hook.
#[derive(Debug, Clone)]
pub struct BuildResult {
    pub system_prompt: String,
    pub messages: Vec<BuildMessage>,
}

/// A single message returned from `build()`.
#[derive(Debug, Clone)]
pub struct BuildMessage {
    pub role: String,
    pub content: String,
}

/// Structured result from a composed Lua nudge function.
#[derive(Debug, Clone)]
pub struct NudgeResult {
    pub text: String,
    pub temporal_fired: bool,
    pub context_pressure_fired: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoSummary {
    pub total: usize,
    pub completed: usize,
    pub incomplete: usize,
}

impl IntoLua for PreTurnState {
    fn into_lua(self, lua: &Lua) -> LuaResult<LuaValue> {
        let table = lua.create_table()?;
        table.set("iteration", self.iteration)?;
        table.set("max_iterations", self.max_iterations)?;
        table.set("remaining", self.remaining)?;
        table.set("elapsed_seconds", self.elapsed_seconds)?;
        table.set("last_input_tokens", self.last_input_tokens)?;
        table.set("context_window", self.context_window)?;
        table.set("temporal_fire_count", self.temporal_fire_count)?;
        table.set("context_pressure_fired", self.context_pressure_fired)?;

        let tc = lua.create_table()?;
        for (k, v) in &self.tool_counts {
            tc.set(k.as_str(), *v)?;
        }
        table.set("tool_counts", tc)?;

        if let Some(todo) = &self.todo_summary {
            let ts = lua.create_table()?;
            ts.set("total", todo.total)?;
            ts.set("completed", todo.completed)?;
            ts.set("incomplete", todo.incomplete)?;
            table.set("todo_summary", ts)?;
        }

        if let Some(ref text) = self.todo_text {
            table.set("todo_text", text.as_str())?;
        }

        Ok(LuaValue::Table(table))
    }
}
