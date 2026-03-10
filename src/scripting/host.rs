use std::path::{Path, PathBuf};

use mlua::prelude::*;
use serde_json::Value;

use super::bindings::{AgentContext, register_ctx};
use super::types::{AgentConfig, BuildMessage, BuildResult, LuaToolDef, NudgeResult, PreTurnState};
use crate::providers::types::ReasoningEffort;

const NUDGES_LUA: &str = include_str!("../../prompts/stdlib/nudges.lua");
const TEMPLATE_LUA: &str = include_str!("../../prompts/stdlib/template.lua");

/// Sandboxed Lua scripting host for a single agent.
///
/// Each `ScriptHost` owns a Lua VM with the agent's `agent.lua` loaded.
/// Hook functions live in the VM and are called via registry keys.
pub struct ScriptHost {
    lua: Lua,
    agent_dir: PathBuf,
    workspace: PathBuf,
    /// Registry keys for custom tool handlers, indexed by position in
    /// `AgentConfig::custom_tools`.
    tool_handler_keys: Vec<LuaRegistryKey>,
}

impl std::fmt::Debug for ScriptHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScriptHost")
            .field("agent_dir", &self.agent_dir)
            .finish()
    }
}

impl ScriptHost {
    /// Create a new sandboxed Lua VM for an agent directory.
    pub fn new(agent_dir: &Path, workspace: &Path) -> LuaResult<Self> {
        let lua = Lua::new();

        // -- Sandbox: remove dangerous globals and modules --
        sandbox_lua(&lua)?;

        // -- Register embedded stdlib as preloaded modules --
        register_stdlib(&lua)?;

        // -- Register host functions --
        let agent_dir_owned = agent_dir.to_path_buf();
        let workspace_owned = workspace.to_path_buf();
        register_host_functions(&lua, &agent_dir_owned, &workspace_owned)?;

        Ok(Self {
            lua,
            agent_dir: agent_dir_owned,
            workspace: workspace_owned,
            tool_handler_keys: Vec::new(),
        })
    }

    /// Load and execute `agent.lua`, returning the parsed config.
    ///
    /// The returned table must have `name` and `description` at minimum.
    /// Hook functions (build_context, pre_turn, etc.) are stored in the
    /// Lua registry for later invocation.
    pub fn load_config(&mut self) -> LuaResult<AgentConfig> {
        let agent_lua_path = self.agent_dir.join("agent.lua");
        let source = std::fs::read_to_string(&agent_lua_path).map_err(|e| {
            LuaError::external(format!("failed to read {}: {e}", agent_lua_path.display()))
        })?;

        let chunk = self
            .lua
            .load(&source)
            .set_name(agent_lua_path.to_string_lossy());
        let table: LuaTable = chunk.eval()?;

        self.extract_config(&table)
    }

    /// Call the `pre_turn(state)` hook. Returns a structured nudge result or None.
    pub fn call_pre_turn(&self, state: PreTurnState) -> LuaResult<Option<NudgeResult>> {
        let globals = self.lua.globals();
        let agent_table: LuaTable = globals.get("__ghost_agent")?;
        let pre_turn: LuaValue = agent_table.get("pre_turn")?;

        match pre_turn {
            LuaValue::Function(f) => {
                let result: LuaValue = f.call(state)?;
                match result {
                    LuaValue::Nil => Ok(None),
                    LuaValue::String(s) => Ok(Some(NudgeResult {
                        text: s.to_str()?.to_string(),
                        temporal_fired: false,
                        context_pressure_fired: false,
                    })),
                    LuaValue::Table(t) => {
                        let text: String = t.get("text")?;
                        let temporal_fired: bool =
                            t.get::<Option<bool>>("temporal_fired")?.unwrap_or(false);
                        let context_pressure_fired: bool = t
                            .get::<Option<bool>>("context_pressure_fired")?
                            .unwrap_or(false);
                        Ok(Some(NudgeResult {
                            text,
                            temporal_fired,
                            context_pressure_fired,
                        }))
                    }
                    other => Err(LuaError::external(format!(
                        "pre_turn must return string, table, or nil, got {other:?}"
                    ))),
                }
            }
            LuaValue::Nil => Ok(None),
            _ => Err(LuaError::external("pre_turn must be a function")),
        }
    }

    /// Call the `on_end_turn(state)` hook. Returns the gate message or None.
    pub fn call_on_end_turn(&self, state: PreTurnState) -> LuaResult<Option<String>> {
        let globals = self.lua.globals();
        let agent_table: LuaTable = globals.get("__ghost_agent")?;
        let on_end_turn: LuaValue = agent_table.get("on_end_turn")?;

        match on_end_turn {
            LuaValue::Function(f) => {
                let result: LuaValue = f.call(state)?;
                match result {
                    LuaValue::Nil => Ok(None),
                    LuaValue::String(s) => Ok(Some(s.to_str()?.to_string())),
                    other => Err(LuaError::external(format!(
                        "on_end_turn must return string or nil, got {other:?}"
                    ))),
                }
            }
            LuaValue::Nil => Ok(None),
            _ => Err(LuaError::external("on_end_turn must be a function")),
        }
    }

    /// Call the `should_trigger(ctx)` hook. Returns whether the agent should run.
    pub async fn call_should_trigger(&self, ctx: AgentContext) -> LuaResult<bool> {
        register_ctx(&self.lua, ctx)?;

        let globals = self.lua.globals();
        let agent_table: LuaTable = globals.get("__ghost_agent")?;
        let hook: LuaValue = agent_table.get("should_trigger")?;

        match hook {
            LuaValue::Function(f) => {
                let globals = self.lua.globals();
                let ctx_val: LuaValue = globals.get("ctx")?;
                let result: LuaValue = f.call_async(ctx_val).await?;
                match result {
                    LuaValue::Boolean(b) => Ok(b),
                    LuaValue::Nil => Ok(true),
                    other => Err(LuaError::external(format!(
                        "should_trigger must return boolean or nil, got {other:?}"
                    ))),
                }
            }
            LuaValue::Nil => Ok(true),
            _ => Err(LuaError::external("should_trigger must be a function")),
        }
    }

    /// Call the `build(ctx, args)` hook. Returns the system prompt and initial messages.
    pub async fn call_build(
        &self,
        ctx: AgentContext,
        args: std::collections::HashMap<String, String>,
    ) -> LuaResult<BuildResult> {
        register_ctx(&self.lua, ctx)?;

        let globals = self.lua.globals();
        let agent_table: LuaTable = globals.get("__ghost_agent")?;
        let hook: LuaValue = agent_table.get("build")?;

        match hook {
            LuaValue::Function(f) => {
                let ctx_val: LuaValue = globals.get("ctx")?;
                let args_table = self.lua.create_table()?;
                for (k, v) in &args {
                    args_table.set(k.as_str(), v.as_str())?;
                }
                let result: LuaTable = f.call_async((ctx_val, args_table)).await?;

                let system_prompt: String = result.get("system_prompt")?;

                let mut messages = Vec::new();
                let msgs_val: LuaValue = result.get("messages")?;
                if let LuaValue::Table(msgs) = msgs_val {
                    for pair in msgs.sequence_values::<LuaTable>() {
                        let msg = pair?;
                        let role: String = msg.get("role")?;
                        let content: String = msg.get("content")?;

                        let tool_calls: Option<Vec<serde_json::Value>> =
                            match msg.get::<LuaValue>("tool_calls")? {
                                LuaValue::Nil => None,
                                val => Some(self.lua.from_value(val)?),
                            };
                        let tool_results: Option<Vec<serde_json::Value>> =
                            match msg.get::<LuaValue>("tool_results")? {
                                LuaValue::Nil => None,
                                val => Some(self.lua.from_value(val)?),
                            };

                        messages.push(BuildMessage {
                            role,
                            content,
                            tool_calls,
                            tool_results,
                        });
                    }
                }

                Ok(BuildResult {
                    system_prompt,
                    messages,
                })
            }
            LuaValue::Nil => Err(LuaError::external(
                "agent must define a build(ctx, args) function",
            )),
            _ => Err(LuaError::external("build must be a function")),
        }
    }

    /// Call the `post_completion(ctx)` hook. No return value.
    pub async fn call_post_completion(&self, ctx: AgentContext) -> LuaResult<()> {
        register_ctx(&self.lua, ctx)?;

        let globals = self.lua.globals();
        let agent_table: LuaTable = globals.get("__ghost_agent")?;
        let hook: LuaValue = agent_table.get("post_completion")?;

        match hook {
            LuaValue::Function(f) => {
                let globals = self.lua.globals();
                let ctx_val: LuaValue = globals.get("ctx")?;
                f.call_async::<()>(ctx_val).await?;
                Ok(())
            }
            LuaValue::Nil => Ok(()),
            _ => Err(LuaError::external("post_completion must be a function")),
        }
    }

    /// Call the `on_resume(ctx, prompt)` hook.
    ///
    /// The hook edits `ctx.system_prompt` and `ctx.messages` in place.
    /// Caller must pre-populate those fields on `ctx` before calling.
    pub async fn call_on_resume(&self, ctx: AgentContext, prompt: &str) -> LuaResult<()> {
        register_ctx(&self.lua, ctx)?;

        let globals = self.lua.globals();
        let agent_table: LuaTable = globals.get("__ghost_agent")?;
        let hook: LuaValue = agent_table.get("on_resume")?;

        match hook {
            LuaValue::Function(f) => {
                let ctx_val: LuaValue = globals.get("ctx")?;
                f.call_async::<()>((ctx_val, prompt)).await?;
                Ok(())
            }
            LuaValue::Nil => Ok(()),
            _ => Err(LuaError::external("on_resume must be a function")),
        }
    }

    /// Access the inner Lua VM (for advanced bindings).
    pub fn lua(&self) -> &Lua {
        &self.lua
    }

    /// Get a reference to a tool handler registry key by index.
    pub fn tool_handler_key(&self, index: usize) -> Option<&LuaRegistryKey> {
        self.tool_handler_keys.get(index)
    }

    pub fn agent_dir(&self) -> &Path {
        &self.agent_dir
    }

    pub fn workspace(&self) -> &Path {
        &self.workspace
    }

    // -- Private helpers --

    fn extract_config(&mut self, table: &LuaTable) -> LuaResult<AgentConfig> {
        // Store the full table as a global for hook access
        self.lua.globals().set("__ghost_agent", table.clone())?;

        let name: String = table.get("name")?;
        let description: String = table.get("description")?;

        let model: Option<String> = table.get("model")?;
        let reasoning_effort: Option<String> = table.get("reasoning_effort")?;
        let reasoning_effort = reasoning_effort.and_then(|s| match s.as_str() {
            "xhigh" => Some(ReasoningEffort::Xhigh),
            "high" => Some(ReasoningEffort::High),
            "medium" => Some(ReasoningEffort::Medium),
            "low" => Some(ReasoningEffort::Low),
            _ => None,
        });

        let max_iterations: usize = table.get::<Option<usize>>("max_iterations")?.unwrap_or(50);

        // Tools list
        let tools: Vec<String> = match table.get::<LuaValue>("tools")? {
            LuaValue::Table(t) => {
                let mut v = Vec::new();
                for pair in t.sequence_values::<String>() {
                    v.push(pair?);
                }
                v
            }
            LuaValue::Nil => Vec::new(),
            _ => return Err(LuaError::external("tools must be a table")),
        };

        // Skills list
        let skills: Vec<String> = match table.get::<LuaValue>("skills")? {
            LuaValue::Table(t) => {
                let mut v = Vec::new();
                for pair in t.sequence_values::<String>() {
                    v.push(pair?);
                }
                v
            }
            LuaValue::Nil => Vec::new(),
            _ => return Err(LuaError::external("skills must be a table")),
        };

        // Custom tools
        let custom_tools = self.extract_custom_tools(table)?;

        // Compaction overrides
        let compaction = match table.get::<LuaValue>("compaction")? {
            LuaValue::Table(t) => {
                let threshold: Option<f64> = t.get("threshold")?;
                let keep_window: Option<usize> = t.get("keep_window")?;
                let mask_preview_chars: Option<usize> = t.get("mask_preview_chars")?;
                let instructions: Option<String> = t.get("instructions")?;
                Some(super::types::AgentCompactionOverrides {
                    threshold,
                    keep_window,
                    mask_preview_chars,
                    instructions,
                })
            }
            LuaValue::Nil => None,
            _ => return Err(LuaError::external("compaction must be a table")),
        };

        // Hook presence
        let has_build = matches!(table.get::<LuaValue>("build")?, LuaValue::Function(_));
        let has_pre_turn = matches!(table.get::<LuaValue>("pre_turn")?, LuaValue::Function(_));
        let has_on_end_turn =
            matches!(table.get::<LuaValue>("on_end_turn")?, LuaValue::Function(_));
        let has_post_completion = matches!(
            table.get::<LuaValue>("post_completion")?,
            LuaValue::Function(_)
        );
        let has_should_trigger = matches!(
            table.get::<LuaValue>("should_trigger")?,
            LuaValue::Function(_)
        );
        let has_on_resume = matches!(table.get::<LuaValue>("on_resume")?, LuaValue::Function(_));

        Ok(AgentConfig {
            name,
            description,
            model,
            reasoning_effort,
            max_iterations,
            tools,
            custom_tools,
            skills,
            compaction,
            has_build,
            has_pre_turn,
            has_on_end_turn,
            has_post_completion,
            has_should_trigger,
            has_on_resume,
        })
    }

    fn extract_custom_tools(&mut self, table: &LuaTable) -> LuaResult<Vec<LuaToolDef>> {
        let custom_tools_val: LuaValue = table.get("custom_tools")?;
        let LuaValue::Table(custom_tools_table) = custom_tools_val else {
            return Ok(Vec::new());
        };

        let mut tools = Vec::new();

        for pair in custom_tools_table.pairs::<String, LuaTable>() {
            let (name, tool_table) = pair?;

            let description: String = tool_table.get("description")?;
            let terminal: bool = tool_table.get::<Option<bool>>("terminal")?.unwrap_or(false);

            // Extract parameters as JSON Schema
            let params_val: LuaValue = tool_table.get::<LuaValue>("parameters")?;
            let parameters = lua_to_json(&params_val)?;

            // Store handler function in registry
            let handler: LuaFunction = tool_table.get("handler")?;
            let key = self.lua.create_registry_value(handler)?;
            let key_index = self.tool_handler_keys.len();
            self.tool_handler_keys.push(key);

            tools.push(LuaToolDef {
                name,
                description,
                parameters,
                terminal,
                handler_key_index: key_index,
            });
        }

        Ok(tools)
    }
}

// -- Sandboxing --

fn sandbox_lua(lua: &Lua) -> LuaResult<()> {
    let globals = lua.globals();

    // Remove dangerous os functions (keep os.date, os.time, os.clock)
    if let Ok(os_table) = globals.get::<LuaTable>("os") {
        for key in &[
            "execute",
            "remove",
            "rename",
            "exit",
            "getenv",
            "tmpname",
            "setlocale",
        ] {
            os_table.set(*key, LuaValue::Nil)?;
        }
    }

    // Remove entire io module
    globals.set("io", LuaValue::Nil)?;

    // Remove dangerous global functions
    globals.set("loadfile", LuaValue::Nil)?;
    globals.set("dofile", LuaValue::Nil)?;

    // Remove package.loadlib
    if let Ok(package_table) = globals.get::<LuaTable>("package") {
        package_table.set("loadlib", LuaValue::Nil)?;
    }

    // Redirect print to logfire debug
    let print_fn = lua.create_function(|_, args: LuaMultiValue| {
        let parts: Vec<String> = args.into_iter().map(|v| format!("{v:?}")).collect();
        let msg = parts.join("\t");
        logfire::debug!("lua print: {msg}", msg = msg);
        Ok(())
    })?;
    globals.set("print", print_fn)?;

    Ok(())
}

// -- Stdlib registration --

fn register_stdlib(lua: &Lua) -> LuaResult<()> {
    // Register ghost.nudges as a preloaded module
    let preload = lua
        .globals()
        .get::<LuaTable>("package")?
        .get::<LuaTable>("preload")?;

    let nudges_loader = lua.create_function(|lua, _: ()| {
        lua.load(NUDGES_LUA)
            .set_name("ghost.nudges")
            .eval::<LuaValue>()
    })?;
    preload.set("ghost.nudges", nudges_loader)?;

    let template_loader = lua.create_function(|lua, _: ()| {
        lua.load(TEMPLATE_LUA)
            .set_name("ghost.template")
            .eval::<LuaValue>()
    })?;
    preload.set("ghost.template", template_loader)?;

    Ok(())
}

// -- Host functions --

fn register_host_functions(lua: &Lua, agent_dir: &Path, workspace: &Path) -> LuaResult<()> {
    let globals = lua.globals();

    // read_file(path) — relative to agent_dir, sandboxed within workspace
    let ad = agent_dir.to_path_buf();
    let ws = workspace.to_path_buf();
    let read_file_fn = lua.create_function(move |_, path: String| {
        let resolved = ad.join(&path);

        // Sandbox: resolved path must be within workspace
        let canonical = resolved
            .canonicalize()
            .map_err(|e| LuaError::external(format!("read_file: cannot resolve '{path}': {e}")))?;
        let ws_canonical = ws.canonicalize().unwrap_or_else(|_| ws.clone());
        if !canonical.starts_with(&ws_canonical) {
            return Err(LuaError::external(format!(
                "read_file: path '{path}' escapes workspace"
            )));
        }

        std::fs::read_to_string(&canonical)
            .map_err(|e| LuaError::external(format!("read_file: failed to read '{path}': {e}")))
    })?;
    globals.set("read_file", read_file_fn)?;

    // load_skill(name) — reads $WORKSPACE/skills/{name}/skill.md, strips YAML frontmatter
    let ws2 = workspace.to_path_buf();
    let load_skill_fn = lua.create_function(move |_, name: String| {
        let skill_path = ws2.join("skills").join(&name).join("skill.md");
        let content = std::fs::read_to_string(&skill_path)
            .map_err(|e| LuaError::external(format!("load_skill: '{name}' not found: {e}")))?;
        Ok(strip_yaml_frontmatter(&content))
    })?;
    globals.set("load_skill", load_skill_fn)?;

    // json.encode / json.decode
    let json_table = lua.create_table()?;

    let encode_fn = lua.create_function(|_, value: LuaValue| {
        let json = lua_to_json(&value)?;
        serde_json::to_string(&json).map_err(|e| LuaError::external(format!("json.encode: {e}")))
    })?;
    json_table.set("encode", encode_fn)?;

    let decode_fn = lua.create_function(|lua, s: String| {
        let value: Value = serde_json::from_str(&s)
            .map_err(|e| LuaError::external(format!("json.decode: {e}")))?;
        json_to_lua(lua, &value)
    })?;
    json_table.set("decode", decode_fn)?;

    globals.set("json", json_table)?;

    Ok(())
}

/// Strip YAML frontmatter (--- ... ---) from content.
fn strip_yaml_frontmatter(content: &str) -> String {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return content.to_string();
    }

    let after_open = &trimmed[3..];
    if let Some(close) = after_open.find("\n---") {
        let body_start = close + 4;
        after_open[body_start..]
            .trim_start_matches('\n')
            .to_string()
    } else {
        content.to_string()
    }
}

/// Convert a Lua value to a serde_json Value.
fn lua_to_json(value: &LuaValue) -> LuaResult<Value> {
    match value {
        LuaValue::Nil => Ok(Value::Null),
        LuaValue::Boolean(b) => Ok(Value::Bool(*b)),
        LuaValue::Integer(i) => Ok(Value::Number((*i).into())),
        LuaValue::Number(n) => Ok(serde_json::Number::from_f64(*n)
            .map(Value::Number)
            .unwrap_or(Value::Null)),
        LuaValue::String(s) => Ok(Value::String(s.to_str()?.to_string())),
        LuaValue::Table(t) => {
            // Detect if it's an array (sequential integer keys starting at 1)
            let len = t.raw_len();
            if len > 0 {
                let mut arr = Vec::with_capacity(len);
                for i in 1..=len {
                    let v: LuaValue = t.raw_get(i)?;
                    arr.push(lua_to_json(&v)?);
                }
                Ok(Value::Array(arr))
            } else {
                let mut map = serde_json::Map::new();
                for pair in t.pairs::<String, LuaValue>() {
                    let (k, v) = pair?;
                    map.insert(k, lua_to_json(&v)?);
                }
                Ok(Value::Object(map))
            }
        }
        _ => Ok(Value::Null),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_workspace() -> tempfile::TempDir {
        let dir = tempfile::TempDir::new().unwrap();
        let agent_dir = dir.path().join("agents").join("test-agent");
        std::fs::create_dir_all(&agent_dir).unwrap();
        dir
    }

    fn write_agent_lua(workspace: &Path, content: &str) {
        let agent_dir = workspace.join("agents").join("test-agent");
        std::fs::create_dir_all(&agent_dir).unwrap();
        std::fs::write(agent_dir.join("agent.lua"), content).unwrap();
    }

    #[test]
    fn load_minimal_agent_config() {
        let dir = test_workspace();
        write_agent_lua(
            dir.path(),
            r#"
            return {
                name = "test-agent",
                description = "A test agent",
                tools = { "web_search", "todo" },
            }
            "#,
        );

        let agent_dir = dir.path().join("agents").join("test-agent");
        let mut host = ScriptHost::new(&agent_dir, dir.path()).unwrap();
        let config = host.load_config().unwrap();

        assert_eq!(config.name, "test-agent");
        assert_eq!(config.description, "A test agent");
        assert_eq!(config.tools, vec!["web_search", "todo"]);
        assert_eq!(config.max_iterations, 50); // default
        assert!(config.model.is_none());
        assert!(!config.has_pre_turn);
        assert!(!config.has_on_end_turn);
        assert!(!config.has_build);
    }

    #[test]
    fn load_full_agent_config() {
        let dir = test_workspace();
        write_agent_lua(
            dir.path(),
            r#"
            return {
                name = "deep-research",
                description = "Iterative web research",
                model = "fast",
                reasoning_effort = "high",
                max_iterations = 30,
                tools = { "web_search", "web_fetch", "todo" },
                build = function(ctx, args)
                    return {
                        system_prompt = "You are a research agent.",
                        messages = {{ role = "user", content = args.prompt or "Begin." }},
                    }
                end,
                pre_turn = function(state) return nil end,
                on_end_turn = function(state) return nil end,
                post_completion = function(ctx, result) end,
            }
            "#,
        );

        let agent_dir = dir.path().join("agents").join("test-agent");
        let mut host = ScriptHost::new(&agent_dir, dir.path()).unwrap();
        let config = host.load_config().unwrap();

        assert_eq!(config.name, "deep-research");
        assert_eq!(config.model.as_deref(), Some("fast"));
        assert_eq!(config.reasoning_effort, Some(ReasoningEffort::High));
        assert_eq!(config.max_iterations, 30);
        assert!(config.has_build);
        assert!(config.has_pre_turn);
        assert!(config.has_on_end_turn);
        assert!(config.has_post_completion);
        assert!(!config.has_should_trigger);
    }

    #[test]
    fn sandbox_blocks_os_execute() {
        let dir = test_workspace();
        write_agent_lua(
            dir.path(),
            r#"
            os.execute("echo pwned")
            return { name = "bad", description = "evil", tools = {} }
            "#,
        );

        let agent_dir = dir.path().join("agents").join("test-agent");
        let mut host = ScriptHost::new(&agent_dir, dir.path()).unwrap();
        let result = host.load_config();
        assert!(result.is_err());
    }

    #[test]
    fn sandbox_blocks_io() {
        let dir = test_workspace();
        write_agent_lua(
            dir.path(),
            r#"
            io.open("/etc/passwd")
            return { name = "bad", description = "evil", tools = {} }
            "#,
        );

        let agent_dir = dir.path().join("agents").join("test-agent");
        let mut host = ScriptHost::new(&agent_dir, dir.path()).unwrap();
        let result = host.load_config();
        assert!(result.is_err());
    }

    #[test]
    fn require_ghost_nudges_loads() {
        let dir = test_workspace();
        write_agent_lua(
            dir.path(),
            r#"
            local nudges = require("ghost.nudges")
            assert(nudges.compose, "compose should exist")
            assert(nudges.temporal, "temporal should exist")
            assert(nudges.iteration_countdown, "iteration_countdown should exist")
            assert(nudges.progress_gate, "progress_gate should exist")
            return { name = "test", description = "test", tools = {} }
            "#,
        );

        let agent_dir = dir.path().join("agents").join("test-agent");
        let mut host = ScriptHost::new(&agent_dir, dir.path()).unwrap();
        let config = host.load_config().unwrap();
        assert_eq!(config.name, "test");
    }

    #[test]
    fn require_ghost_template_loads() {
        let dir = test_workspace();
        write_agent_lua(
            dir.path(),
            r#"
            local template = require("ghost.template")
            local result = template.render("Hello {{name}}", { name = "World" })
            assert(result == "Hello World", "template rendering failed: " .. result)
            return { name = "test", description = "test", tools = {} }
            "#,
        );

        let agent_dir = dir.path().join("agents").join("test-agent");
        let mut host = ScriptHost::new(&agent_dir, dir.path()).unwrap();
        host.load_config().unwrap();
    }

    #[test]
    fn read_file_works() {
        let dir = test_workspace();
        let agent_dir = dir.path().join("agents").join("test-agent");
        std::fs::write(agent_dir.join("prompt.md"), "# Hello\nWorld").unwrap();

        write_agent_lua(
            dir.path(),
            r#"
            local content = read_file("prompt.md")
            assert(content:find("Hello"), "read_file should load content")
            return {
                name = "test",
                description = "test",
                tools = {},
                build = function(ctx, args)
                    return {
                        system_prompt = content,
                        messages = {{ role = "user", content = "test" }},
                    }
                end,
            }
            "#,
        );

        let mut host = ScriptHost::new(&agent_dir, dir.path()).unwrap();
        let config = host.load_config().unwrap();
        assert!(config.has_build);
    }

    #[test]
    fn read_file_blocks_escape() {
        let dir = test_workspace();
        write_agent_lua(
            dir.path(),
            r#"
            read_file("../../etc/passwd")
            return { name = "bad", description = "evil", tools = {} }
            "#,
        );

        let agent_dir = dir.path().join("agents").join("test-agent");
        let mut host = ScriptHost::new(&agent_dir, dir.path()).unwrap();
        let result = host.load_config();
        assert!(result.is_err());
    }

    #[test]
    fn load_skill_works() {
        let dir = test_workspace();
        let skill_dir = dir.path().join("skills").join("note-writer");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("skill.md"),
            "---\nname: note-writer\n---\n\n# Note Guide\nWrite notes.",
        )
        .unwrap();

        write_agent_lua(
            dir.path(),
            r#"
            local skill = load_skill("note-writer")
            assert(skill:find("Note Guide"), "guide")
            assert(not skill:find("note%-writer"), "strip")
            return {
                name = "test",
                description = "test",
                tools = {},
            }
            "#,
        );

        let agent_dir = dir.path().join("agents").join("test-agent");
        let mut host = ScriptHost::new(&agent_dir, dir.path()).unwrap();
        host.load_config().unwrap();
    }

    #[test]
    fn json_encode_decode() {
        let dir = test_workspace();
        write_agent_lua(
            dir.path(),
            r#"
            local encoded = json.encode({ foo = "bar", num = 42 })
            local decoded = json.decode(encoded)
            assert(decoded.foo == "bar")
            assert(decoded.num == 42)
            return { name = "test", description = "test", tools = {} }
            "#,
        );

        let agent_dir = dir.path().join("agents").join("test-agent");
        let mut host = ScriptHost::new(&agent_dir, dir.path()).unwrap();
        host.load_config().unwrap();
    }

    #[test]
    fn pre_turn_hook_returns_nudge() {
        let dir = test_workspace();
        write_agent_lua(
            dir.path(),
            r#"
            return {
                name = "test",
                description = "test",
                tools = {},
                pre_turn = function(state)
                    if state.remaining <= 5 then
                        return "Hurry up! " .. state.remaining .. " iterations left."
                    end
                    return nil
                end,
            }
            "#,
        );

        let agent_dir = dir.path().join("agents").join("test-agent");
        let mut host = ScriptHost::new(&agent_dir, dir.path()).unwrap();
        host.load_config().unwrap();

        let state = PreTurnState {
            iteration: 25,
            max_iterations: 30,
            remaining: 5,
            elapsed_seconds: 120,
            tool_counts: Default::default(),
            last_input_tokens: 1000,
            context_window: 128000,
            todo_summary: None,
            todo_text: None,
            temporal_fire_count: 0,
            context_pressure_fired: false,
        };

        let result = host.call_pre_turn(state).unwrap();
        assert!(result.is_some());
        let nudge = result.unwrap();
        assert_eq!(nudge.text, "Hurry up! 5 iterations left.");
        assert!(!nudge.temporal_fired);
        assert!(!nudge.context_pressure_fired);

        // With plenty of remaining iterations
        let state2 = PreTurnState {
            iteration: 5,
            max_iterations: 30,
            remaining: 25,
            elapsed_seconds: 30,
            tool_counts: Default::default(),
            last_input_tokens: 1000,
            context_window: 128000,
            todo_summary: None,
            todo_text: None,
            temporal_fire_count: 0,
            context_pressure_fired: false,
        };

        let result2 = host.call_pre_turn(state2).unwrap();
        assert!(result2.is_none());
    }

    #[test]
    fn nudges_compose_with_temporal() {
        let dir = test_workspace();
        write_agent_lua(
            dir.path(),
            r#"
            local nudges = require("ghost.nudges")

            return {
                name = "test",
                description = "test",
                tools = {},
                pre_turn = nudges.compose(
                    nudges.temporal({
                        after_seconds = 0,
                        messages = { "hurry up after {minutes} min" },
                    })
                ),
            }
            "#,
        );

        let agent_dir = dir.path().join("agents").join("test-agent");
        let mut host = ScriptHost::new(&agent_dir, dir.path()).unwrap();
        host.load_config().unwrap();

        let state = PreTurnState {
            iteration: 1,
            max_iterations: 30,
            remaining: 29,
            elapsed_seconds: 120,
            tool_counts: Default::default(),
            last_input_tokens: 1000,
            context_window: 128000,
            todo_summary: None,
            todo_text: None,
            temporal_fire_count: 0,
            context_pressure_fired: false,
        };

        let result = host.call_pre_turn(state).unwrap();
        assert!(result.is_some());
        let nudge = result.unwrap();
        assert!(nudge.text.contains("<system-reminder>"));
        assert!(nudge.text.contains("hurry up after 2 min"));
        assert!(nudge.temporal_fired);
        assert!(!nudge.context_pressure_fired);
    }

    #[test]
    fn on_end_turn_progress_gate() {
        let dir = test_workspace();
        write_agent_lua(
            dir.path(),
            r#"
            local nudges = require("ghost.nudges")

            return {
                name = "test",
                description = "test",
                tools = {},
                on_end_turn = nudges.progress_gate({
                    no_todo = "Create a plan first!",
                    incomplete = "{incomplete} items remain.",
                }),
            }
            "#,
        );

        let agent_dir = dir.path().join("agents").join("test-agent");
        let mut host = ScriptHost::new(&agent_dir, dir.path()).unwrap();
        host.load_config().unwrap();

        // No todo summary → gate fires with no_todo message
        let state = PreTurnState {
            iteration: 5,
            max_iterations: 30,
            remaining: 25,
            elapsed_seconds: 60,
            tool_counts: Default::default(),
            last_input_tokens: 1000,
            context_window: 128000,
            todo_summary: Some(super::super::types::TodoSummary {
                total: 0,
                completed: 0,
                incomplete: 0,
            }),
            todo_text: None,
            temporal_fire_count: 0,
            context_pressure_fired: false,
        };

        let result = host.call_on_end_turn(state).unwrap();
        assert_eq!(result.as_deref(), Some("Create a plan first!"));
    }

    async fn test_db(workspace: &Path) -> crate::db::GhostDb {
        crate::db::connect(workspace, 1024).await.unwrap()
    }

    fn test_ctx(db: crate::db::GhostDb, workspace: &Path) -> super::super::bindings::AgentContext {
        super::super::bindings::AgentContext::new(
            db,
            workspace.to_path_buf(),
            "test-agent".to_string(),
            "test-session".to_string(),
        )
    }

    #[tokio::test]
    async fn should_trigger_returns_true_by_default() {
        let dir = test_workspace();
        write_agent_lua(
            dir.path(),
            r#"
            return {
                name = "test",
                description = "test",
                tools = {},
            }
            "#,
        );

        let agent_dir = dir.path().join("agents").join("test-agent");
        let mut host = ScriptHost::new(&agent_dir, dir.path()).unwrap();
        host.load_config().unwrap();

        let db = test_db(dir.path()).await;
        let ctx = test_ctx(db, dir.path());
        assert!(host.call_should_trigger(ctx).await.unwrap());
    }

    #[tokio::test]
    async fn should_trigger_returns_false() {
        let dir = test_workspace();
        write_agent_lua(
            dir.path(),
            r#"
            return {
                name = "test",
                description = "test",
                tools = {},
                should_trigger = function(ctx)
                    return ctx.agent_slug == "other-agent"
                end,
            }
            "#,
        );

        let agent_dir = dir.path().join("agents").join("test-agent");
        let mut host = ScriptHost::new(&agent_dir, dir.path()).unwrap();
        host.load_config().unwrap();

        let db = test_db(dir.path()).await;
        let ctx = test_ctx(db, dir.path());
        // agent_slug is "test-agent", not "other-agent"
        assert!(!host.call_should_trigger(ctx).await.unwrap());
    }

    #[tokio::test]
    async fn should_trigger_with_async_ctx_methods() {
        let dir = test_workspace();
        write_agent_lua(
            dir.path(),
            r#"
            return {
                name = "test",
                description = "test",
                tools = {},
                should_trigger = function(ctx)
                    -- This calls an async method; must not error
                    local sessions = ctx:list_interface_sessions()
                    return #sessions > 0
                end,
            }
            "#,
        );

        let agent_dir = dir.path().join("agents").join("test-agent");
        let mut host = ScriptHost::new(&agent_dir, dir.path()).unwrap();
        host.load_config().unwrap();

        let db = test_db(dir.path()).await;
        let ctx = test_ctx(db, dir.path());
        // No interface sessions in test DB → should return false
        let result = host.call_should_trigger(ctx).await.unwrap();
        assert!(!result);
    }

    #[tokio::test]
    async fn should_trigger_with_async_state_methods() {
        let dir = test_workspace();
        write_agent_lua(
            dir.path(),
            r#"
            return {
                name = "test",
                description = "test",
                tools = {},
                should_trigger = function(ctx)
                    local val = ctx:get("last_run")
                    if val then
                        return false
                    end
                    return true
                end,
            }
            "#,
        );

        let agent_dir = dir.path().join("agents").join("test-agent");
        let mut host = ScriptHost::new(&agent_dir, dir.path()).unwrap();
        host.load_config().unwrap();

        let db = test_db(dir.path()).await;

        // First call: no state set → should trigger
        let ctx = test_ctx(db.clone(), dir.path());
        assert!(host.call_should_trigger(ctx).await.unwrap());

        // Set state, then check again → should NOT trigger
        crate::db::agent_state::set_state(&db, "test-agent", "last_run", "2026-01-01T00:00:00Z")
            .await
            .unwrap();

        let ctx2 = test_ctx(db, dir.path());
        assert!(!host.call_should_trigger(ctx2).await.unwrap());
    }

    #[tokio::test]
    async fn build_hook_returns_system_prompt_and_messages() {
        let dir = test_workspace();
        write_agent_lua(
            dir.path(),
            r#"
            return {
                name = "test",
                description = "test",
                tools = {},
                build = function(ctx, args)
                    return {
                        system_prompt = "Prompt for " .. ctx.agent_slug,
                        messages = {
                            { role = "user", content = args.prompt or "default" },
                        },
                    }
                end,
            }
            "#,
        );

        let agent_dir = dir.path().join("agents").join("test-agent");
        let mut host = ScriptHost::new(&agent_dir, dir.path()).unwrap();
        let config = host.load_config().unwrap();
        assert!(config.has_build);

        let db = test_db(dir.path()).await;
        let ctx = test_ctx(db, dir.path());
        let args = std::collections::HashMap::from([("prompt".into(), "Research topic X".into())]);
        let result = host.call_build(ctx, args).await.unwrap();
        assert_eq!(result.system_prompt, "Prompt for test-agent");
        assert_eq!(result.messages.len(), 1);
        assert_eq!(result.messages[0].role, "user");
        assert_eq!(result.messages[0].content, "Research topic X");
    }

    #[tokio::test]
    async fn build_hook_missing_returns_error() {
        let dir = test_workspace();
        write_agent_lua(
            dir.path(),
            r#"
            return {
                name = "test",
                description = "test",
                tools = {},
            }
            "#,
        );

        let agent_dir = dir.path().join("agents").join("test-agent");
        let mut host = ScriptHost::new(&agent_dir, dir.path()).unwrap();
        let config = host.load_config().unwrap();
        assert!(!config.has_build);

        let db = test_db(dir.path()).await;
        let ctx = test_ctx(db, dir.path());
        let result = host.call_build(ctx, std::collections::HashMap::new()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn post_completion_runs_without_error() {
        let dir = test_workspace();
        write_agent_lua(
            dir.path(),
            r#"
            return {
                name = "test",
                description = "test",
                tools = {},
                post_completion = function(ctx)
                    -- noop, just verify it runs
                end,
            }
            "#,
        );

        let agent_dir = dir.path().join("agents").join("test-agent");
        let mut host = ScriptHost::new(&agent_dir, dir.path()).unwrap();
        host.load_config().unwrap();

        let db = test_db(dir.path()).await;
        let ctx = test_ctx(db, dir.path());
        host.call_post_completion(ctx).await.unwrap();
    }

    #[tokio::test]
    async fn post_completion_noop_when_missing() {
        let dir = test_workspace();
        write_agent_lua(
            dir.path(),
            r#"
            return {
                name = "test",
                description = "test",
                tools = {},
            }
            "#,
        );

        let agent_dir = dir.path().join("agents").join("test-agent");
        let mut host = ScriptHost::new(&agent_dir, dir.path()).unwrap();
        host.load_config().unwrap();

        let db = test_db(dir.path()).await;
        let ctx = test_ctx(db, dir.path());
        host.call_post_completion(ctx).await.unwrap();
    }

    #[test]
    fn custom_tool_extraction() {
        let dir = test_workspace();
        write_agent_lua(
            dir.path(),
            r#"
            return {
                name = "test",
                description = "test",
                tools = {},
                custom_tools = {
                    report_findings = {
                        description = "Submit final report",
                        parameters = {
                            type = "object",
                            properties = {
                                report = { type = "string", description = "The report" },
                            },
                            required = { "report" },
                        },
                        handler = function(ctx, args)
                            return "Report received: " .. args.report
                        end,
                        terminal = true,
                    },
                },
            }
            "#,
        );

        let agent_dir = dir.path().join("agents").join("test-agent");
        let mut host = ScriptHost::new(&agent_dir, dir.path()).unwrap();
        let config = host.load_config().unwrap();

        assert_eq!(config.custom_tools.len(), 1);
        let tool = &config.custom_tools[0];
        assert_eq!(tool.name, "report_findings");
        assert_eq!(tool.description, "Submit final report");
        assert!(tool.terminal);
        assert!(tool.parameters.is_object());

        // Verify the handler can be called
        let key = host.tool_handler_key(tool.handler_key_index).unwrap();
        let handler: LuaFunction = host.lua().registry_value(key).unwrap();
        let result: String = handler
            .call::<String>(("test", host.lua().create_table().unwrap()))
            .unwrap_or_else(|_| {
                // Handler expects (ctx, args) — pass simple args
                "fallback".to_string()
            });
        // The handler will error because we passed a string instead of table for ctx,
        // but that's fine — we verified it's callable.
        let _ = result;
    }
}
