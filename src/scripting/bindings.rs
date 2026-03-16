use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use mlua::prelude::*;

use crate::config::Config;
use crate::db::GhostDb;
use crate::tools::manager::ToolManager;

/// A request to spawn a child agent, accumulated during post_completion.
#[derive(Debug, Clone)]
pub struct SpawnRequest {
    pub agent: String,
    pub args: HashMap<String, String>,
}

/// A message in the resume context (editable from Lua).
#[derive(Debug, Clone)]
pub struct LuaMessage {
    pub role: String,
    pub content: String,
}

/// Context object exposed to Lua hooks as `ctx` userdata.
///
/// Provides access to agent state, database queries, and workspace info.
#[derive(Debug, Clone)]
pub struct AgentContext {
    pub db: GhostDb,
    pub workspace: PathBuf,
    pub agent_slug: String,
    pub session_id: String,
    pub trigger_session_id: Option<String>,
    pub spawn_requests: Arc<Mutex<Vec<SpawnRequest>>>,
    /// Editable system prompt for `on_resume` hook.
    pub system_prompt: Arc<Mutex<Option<String>>>,
    /// Editable message history for `on_resume` hook.
    pub resume_messages: Arc<Mutex<Option<Vec<LuaMessage>>>>,
    /// Config for tool execution via `ctx:call_tool()`.
    pub config: Option<Config>,
    /// Tool manager for `ctx:call_tool()` / `ctx:call_tools()`.
    pub tool_manager: Option<Arc<ToolManager>>,
}

impl AgentContext {
    pub fn new(db: GhostDb, workspace: PathBuf, agent_slug: String, session_id: String) -> Self {
        Self {
            db,
            workspace,
            agent_slug,
            session_id,
            trigger_session_id: None,
            spawn_requests: Arc::new(Mutex::new(Vec::new())),
            system_prompt: Arc::new(Mutex::new(None)),
            resume_messages: Arc::new(Mutex::new(None)),
            config: None,
            tool_manager: None,
        }
    }

    /// Enable `ctx:call_tool()` / `ctx:call_tools()` by attaching config and tool manager.
    pub fn with_tool_support(mut self, config: Config, tool_manager: Arc<ToolManager>) -> Self {
        self.config = Some(config);
        self.tool_manager = Some(tool_manager);
        self
    }
}

impl LuaUserData for AgentContext {
    fn add_fields<F: LuaUserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("session_id", |_, this| Ok(this.session_id.clone()));
        fields.add_field_method_get("agent_slug", |_, this| Ok(this.agent_slug.clone()));
        fields.add_field_method_get("trigger_session_id", |_, this| {
            Ok(this.trigger_session_id.clone())
        });
        fields.add_field_method_get("workspace", |_, this| {
            Ok(this.workspace.display().to_string())
        });

        // ctx.system_prompt — get/set for on_resume hook
        fields.add_field_method_get("system_prompt", |_, this| {
            let guard = this.system_prompt.lock().expect("system_prompt lock");
            Ok(guard.clone())
        });
        fields.add_field_method_set("system_prompt", |_, this, val: String| {
            let mut guard = this.system_prompt.lock().expect("system_prompt lock");
            *guard = Some(val);
            Ok(())
        });

        // ctx.messages — get/set for on_resume hook
        fields.add_field_method_get("messages", |lua, this| {
            let guard = this.resume_messages.lock().expect("resume_messages lock");
            match guard.as_ref() {
                None => Ok(LuaValue::Nil),
                Some(msgs) => {
                    let table = lua.create_table()?;
                    for (i, msg) in msgs.iter().enumerate() {
                        let row = lua.create_table()?;
                        row.set("role", msg.role.as_str())?;
                        row.set("content", msg.content.as_str())?;
                        table.raw_set(i + 1, row)?;
                    }
                    Ok(LuaValue::Table(table))
                }
            }
        });
        fields.add_field_method_set("messages", |_, this, val: LuaTable| {
            let mut msgs = Vec::new();
            for pair in val.sequence_values::<LuaTable>() {
                let row = pair?;
                let role: String = row.get("role")?;
                let content: String = row.get("content")?;
                msgs.push(LuaMessage { role, content });
            }
            let mut guard = this.resume_messages.lock().expect("resume_messages lock");
            *guard = Some(msgs);
            Ok(())
        });
    }

    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        // ctx:get(key) -> string|nil
        methods.add_async_method("get", |_, this, key: String| async move {
            let result = crate::db::agent_state::get_state(&this.db, &this.agent_slug, &key)
                .await
                .map_err(|e| LuaError::external(e.to_string()))?;
            Ok(result)
        });

        // ctx:set(key, value)
        methods.add_async_method(
            "set",
            |_, this, (key, value): (String, String)| async move {
                crate::db::agent_state::set_state(&this.db, &this.agent_slug, &key, &value)
                    .await
                    .map_err(|e| LuaError::external(e.to_string()))?;
                Ok(())
            },
        );

        // ctx:delete(key)
        methods.add_async_method("delete", |_, this, key: String| async move {
            crate::db::agent_state::delete_state(&this.db, &this.agent_slug, &key)
                .await
                .map_err(|e| LuaError::external(e.to_string()))?;
            Ok(())
        });

        // ctx:count_messages_since(session_id, rfc3339_string) -> number
        methods.add_async_method(
            "count_messages_since",
            |_, this, (sid, since): (String, String)| async move {
                let dt = chrono::DateTime::parse_from_rfc3339(&since)
                    .map_err(|e| LuaError::external(format!("invalid rfc3339 date: {e}")))?
                    .with_timezone(&chrono::Utc);
                let count = crate::db::sessions::count_messages_since(&this.db, &sid, &dt)
                    .await
                    .map_err(|e| LuaError::external(e.to_string()))?;
                Ok(count)
            },
        );

        // ctx:list_interface_sessions() -> [{interface, session_id}]
        methods.add_async_method("list_interface_sessions", |lua, this, ()| async move {
            let sessions = crate::db::interface_sessions::list_all_interface_sessions(&this.db)
                .await
                .map_err(|e| LuaError::external(e.to_string()))?;
            let table = lua.create_table()?;
            for (i, s) in sessions.iter().enumerate() {
                let row = lua.create_table()?;
                row.set("interface", s.interface.clone())?;
                row.set("session_id", s.session_id.clone())?;
                table.raw_set(i + 1, row)?;
            }
            Ok(table)
        });

        // ctx:filter_transcript(session_id, since?) -> string
        methods.add_async_method(
            "filter_transcript",
            |_, this, (sid, since): (String, Option<String>)| async move {
                let messages = crate::db::sessions::list_messages_by_session(&this.db, &sid)
                    .await
                    .map_err(|e| LuaError::external(e.to_string()))?;
                Ok(crate::chat::filter_transcript(&messages, since.as_deref()))
            },
        );

        // ctx:load_diary_today() -> string|nil
        methods.add_method("load_diary_today", |_, this, ()| {
            Ok(crate::knowledge::load_diary_today(&this.workspace))
        });

        // ctx:list_messages(session_id) -> [{role, content, created_at}]
        methods.add_async_method("list_messages", |lua, this, sid: String| async move {
            let messages = crate::db::sessions::list_messages_by_session(&this.db, &sid)
                .await
                .map_err(|e| LuaError::external(e.to_string()))?;
            let table = lua.create_table()?;
            for (i, msg) in messages.iter().enumerate() {
                let row = lua.create_table()?;
                row.set("role", msg.role.clone())?;
                row.set("content", msg.content.clone())?;
                row.set("created_at", msg.created_at.clone())?;
                table.raw_set(i + 1, row)?;
            }
            Ok(table)
        });

        // ctx:curate_web_cache(session_id?) -> {moved=N, deleted=N, edges=N}
        methods.add_async_method(
            "curate_web_cache",
            |lua, this, sid: Option<String>| async move {
                let session_id = sid.as_deref().unwrap_or(&this.session_id);
                let messages = crate::db::sessions::list_messages_by_session(&this.db, session_id)
                    .await
                    .map_err(|e| LuaError::external(e.to_string()))?;

                let agent_findings = crate::chat::extract_agent_findings(&messages);

                let classified = crate::web::classify_web_cache(
                    &this.workspace,
                    session_id,
                    agent_findings.as_deref(),
                    1000,
                );

                let curation =
                    crate::web::curate_references(&this.workspace, session_id, &classified);

                let edges =
                    crate::web::link_cited_edges(&this.db, &this.workspace, &classified).await;

                let result = lua.create_table()?;
                result.set("moved", curation.moved)?;
                result.set("deleted", curation.deleted)?;
                result.set("edges", edges)?;
                Ok(result)
            },
        );

        // ctx:call_tool(name, args_table) -> string
        methods.add_async_method(
            "call_tool",
            |lua, this, (name, args): (String, LuaValue)| async move {
                let config = this.config.as_ref().ok_or_else(|| {
                    LuaError::external("call_tool not available: agent context has no tool support")
                })?;
                let tool_manager = this.tool_manager.as_ref().ok_or_else(|| {
                    LuaError::external("call_tool not available: agent context has no tool manager")
                })?;

                let params: serde_json::Value = lua.from_value(args)?;

                // NOTE: Each script context gets a fresh browser session.
                // Lua agent tool calls don't persist browser state across calls.
                let tool_ctx = crate::tools::context::ToolContext {
                    workspace: this.workspace.clone(),
                    cwd: this.workspace.clone(),
                    db: this.db.clone(),
                    config: config.clone(),
                    session_id: this.session_id.clone(),
                    agent_runner: None,
                    event_tx: None,
                    channel_id: None,
                    confirmation_tx: None,
                    browser_manager: std::sync::Arc::new(tokio::sync::Mutex::new(
                        crate::web::browser::BrowserManager::new(vec![]),
                    )),
                };

                let output = tool_manager
                    .execute(&name, params, &tool_ctx)
                    .await
                    .map_err(|e| LuaError::external(format!("call_tool({name}) failed: {e}")))?;

                Ok(output.text)
            },
        );

        // ctx:call_tools({{name, args}, ...}) -> list of message tables
        // Tools are executed in parallel for performance.
        methods.add_async_method("call_tools", |lua, this, calls: LuaTable| async move {
            let config = this
                .config
                .as_ref()
                .ok_or_else(|| LuaError::external("call_tools not available: no tool support"))?;
            let tool_manager = this
                .tool_manager
                .as_ref()
                .ok_or_else(|| LuaError::external("call_tools not available: no tool manager"))?;

            // Extract call data from Lua table before any await points
            // (LuaTable is not Send)
            let mut parsed_calls: Vec<(String, serde_json::Value)> = Vec::new();
            for pair in calls.sequence_values::<LuaTable>() {
                let entry = pair?;
                let name: String = entry.get(1)?;
                let args: LuaValue = entry.get(2)?;
                let params: serde_json::Value = lua.from_value(args)?;
                parsed_calls.push((name, params));
            }

            // NOTE: Each script context gets a fresh browser session.
            // Lua agent tool calls don't persist browser state across calls.
            let tool_ctx = crate::tools::context::ToolContext {
                workspace: this.workspace.clone(),
                cwd: this.workspace.clone(),
                db: this.db.clone(),
                config: config.clone(),
                session_id: this.session_id.clone(),
                agent_runner: None,
                event_tx: None,
                channel_id: None,
                confirmation_tx: None,
                browser_manager: std::sync::Arc::new(tokio::sync::Mutex::new(
                    crate::web::browser::BrowserManager::new(vec![]),
                )),
            };

            // Build tool_calls_json upfront (just metadata, no execution)
            let tool_calls_json: Vec<serde_json::Value> = parsed_calls
                .iter()
                .enumerate()
                .map(|(i, (name, params))| {
                    serde_json::json!({
                        "id": format!("build_{i}"),
                        "name": name,
                        "input": params,
                    })
                })
                .collect();

            // Execute all tools in parallel
            let futures: Vec<_> = parsed_calls
                .into_iter()
                .enumerate()
                .map(|(i, (name, params))| {
                    let call_id = format!("build_{i}");
                    let tm = Arc::clone(tool_manager);
                    let ctx = tool_ctx.clone();
                    async move {
                        match tm.execute(&name, params, &ctx).await {
                            Ok(output) => serde_json::json!({
                                "tool_use_id": call_id,
                                "content": output.text,
                                "is_error": false,
                            }),
                            Err(e) => serde_json::json!({
                                "tool_use_id": call_id,
                                "content": format!("Error: {e}"),
                                "is_error": true,
                            }),
                        }
                    }
                })
                .collect();
            let tool_results_json = futures::future::join_all(futures).await;

            // Build two Lua table messages
            let messages = lua.create_table()?;

            let assistant_msg = lua.create_table()?;
            assistant_msg.set("role", "assistant")?;
            assistant_msg.set("content", "")?;
            assistant_msg.set("tool_calls", lua.to_value(&tool_calls_json)?)?;
            messages.raw_set(1, assistant_msg)?;

            let results_msg = lua.create_table()?;
            results_msg.set("role", "user")?;
            results_msg.set("content", "")?;
            results_msg.set("tool_results", lua.to_value(&tool_results_json)?)?;
            messages.raw_set(2, results_msg)?;

            Ok(messages)
        });

        // ctx:spawn_agent(name, args_table)
        methods.add_method(
            "spawn_agent",
            |_, this, (name, args): (String, LuaTable)| {
                let mut map = HashMap::new();
                for pair in args.pairs::<String, String>() {
                    let (k, v) = pair?;
                    map.insert(k, v);
                }
                this.spawn_requests
                    .lock()
                    .expect("spawn_requests lock poisoned")
                    .push(SpawnRequest {
                        agent: name,
                        args: map,
                    });
                Ok(())
            },
        );
    }
}

/// Register the AgentContext on a ScriptHost's Lua VM as a global `ctx`.
pub fn register_ctx(lua: &Lua, ctx: AgentContext) -> LuaResult<()> {
    lua.globals().set("ctx", ctx)?;
    Ok(())
}
