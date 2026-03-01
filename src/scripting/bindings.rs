use std::path::PathBuf;

use mlua::prelude::*;

use crate::db::GhostDb;

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
    pub trigger_agent_name: Option<String>,
}

impl LuaUserData for AgentContext {
    fn add_fields<F: LuaUserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("session_id", |_, this| Ok(this.session_id.clone()));
        fields.add_field_method_get("agent_slug", |_, this| Ok(this.agent_slug.clone()));
        fields.add_field_method_get("trigger_session_id", |_, this| {
            Ok(this.trigger_session_id.clone())
        });
        fields.add_field_method_get("trigger_agent_name", |_, this| {
            Ok(this.trigger_agent_name.clone())
        });
        fields.add_field_method_get("workspace", |_, this| {
            Ok(this.workspace.display().to_string())
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

        // ctx:curate_web_cache() -> {moved=N, deleted=N, edges=N}
        methods.add_async_method("curate_web_cache", |lua, this, ()| async move {
            let messages =
                crate::db::sessions::list_messages_by_session(&this.db, &this.session_id)
                    .await
                    .map_err(|e| LuaError::external(e.to_string()))?;

            let agent_findings = crate::reflection::extract_agent_findings(&messages);

            let classified = crate::reflection::classify_web_cache(
                &this.workspace,
                &this.session_id,
                agent_findings.as_deref(),
                1000,
            );

            let curation = crate::reflection::curate_references(
                &this.workspace,
                &this.session_id,
                &classified,
            );

            let edges =
                crate::reflection::link_cited_edges(&this.db, &this.workspace, &classified).await;

            let result = lua.create_table()?;
            result.set("moved", curation.moved)?;
            result.set("deleted", curation.deleted)?;
            result.set("edges", edges)?;
            Ok(result)
        });
    }
}

/// Register the AgentContext on a ScriptHost's Lua VM as a global `ctx`.
pub fn register_ctx(lua: &Lua, ctx: AgentContext) -> LuaResult<()> {
    lua.globals().set("ctx", ctx)?;
    Ok(())
}
