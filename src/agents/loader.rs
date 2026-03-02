use std::path::Path;

use crate::scripting::host::ScriptHost;
use crate::scripting::types::AgentConfig;

use super::error::AgentError;

/// Minimal metadata for listing agents in the system prompt.
#[derive(Debug, Clone)]
pub struct AgentInfo {
    pub name: String,
    pub description: String,
}

/// Scan `$WORKSPACE/agents/` for agent definition folders and return a
/// sorted list of agent name + description pairs.
///
/// Looks for directories containing `agent.lua`. Falls back to scanning
/// `.md` files for backward compatibility during migration.
#[tracing::instrument(skip_all, level = "debug", fields(workspace = %workspace.display()))]
pub fn discover_agents(workspace: &Path) -> Vec<AgentInfo> {
    let agents_dir = workspace.join("agents");

    let entries = match std::fs::read_dir(&agents_dir) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };

    let mut agents = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();

        // Lua folder agent: agents/{name}/agent.lua
        if path.is_dir() {
            let agent_lua = path.join("agent.lua");
            if agent_lua.exists() {
                match load_agent(workspace, &entry.file_name().to_string_lossy()) {
                    Ok(config) => agents.push(AgentInfo {
                        name: config.name,
                        description: config.description,
                    }),
                    Err(e) => {
                        logfire::warn!(
                            "Malformed agent.lua in {path}: {error}",
                            path = path.display().to_string(),
                            error = e.to_string(),
                        );
                    }
                }
            }
        }
    }

    agents.sort_by(|a, b| a.name.cmp(&b.name));
    agents
}

/// Load an agent's config from its Lua folder. Lightweight — drops the VM
/// after extracting the config.
#[tracing::instrument(skip_all, fields(agent_name = name))]
pub fn load_agent(workspace: &Path, name: &str) -> Result<AgentConfig, AgentError> {
    let agent_dir = workspace.join("agents").join(name);
    let agent_lua = agent_dir.join("agent.lua");

    if !agent_lua.exists() {
        return Err(AgentError::NotFound {
            name: name.to_string(),
        });
    }

    let mut host = ScriptHost::new(&agent_dir, workspace).map_err(|e| AgentError::ScriptError {
        agent: name.to_string(),
        message: e.to_string(),
    })?;

    host.load_config().map_err(|e| AgentError::ScriptError {
        agent: name.to_string(),
        message: e.to_string(),
    })
}

/// Load an agent's config and keep the ScriptHost alive for hook execution.
#[tracing::instrument(skip_all, fields(agent_name = name))]
pub fn load_agent_with_host(
    workspace: &Path,
    name: &str,
) -> Result<(AgentConfig, ScriptHost), AgentError> {
    let agent_dir = workspace.join("agents").join(name);
    let agent_lua = agent_dir.join("agent.lua");

    if !agent_lua.exists() {
        return Err(AgentError::NotFound {
            name: name.to_string(),
        });
    }

    let mut host = ScriptHost::new(&agent_dir, workspace).map_err(|e| AgentError::ScriptError {
        agent: name.to_string(),
        message: e.to_string(),
    })?;

    let config = host.load_config().map_err(|e| AgentError::ScriptError {
        agent: name.to_string(),
        message: e.to_string(),
    })?;

    Ok((config, host))
}

/// LuaLS type stubs for agent developers.
const LUA_TYPE_STUBS: &str = include_str!("../../prompts/types/ghost.lua");

/// Embedded default agent files (agent.lua + prompt.md per agent).
const DEFAULT_AGENTS: &[(&str, &[(&str, &str)])] = &[
    (
        "deep-research",
        &[
            (
                "agent.lua",
                include_str!("../../prompts/agents/deep-research/agent.lua"),
            ),
            (
                "prompt.md",
                include_str!("../../prompts/agents/deep-research/prompt.md"),
            ),
        ],
    ),
    (
        "fork-reflection",
        &[
            (
                "agent.lua",
                include_str!("../../prompts/agents/fork-reflection/agent.lua"),
            ),
            (
                "prompt.md",
                include_str!("../../prompts/agents/fork-reflection/prompt.md"),
            ),
        ],
    ),
    (
        "chat-reflection",
        &[
            (
                "agent.lua",
                include_str!("../../prompts/agents/chat-reflection/agent.lua"),
            ),
            (
                "prompt.md",
                include_str!("../../prompts/agents/chat-reflection/prompt.md"),
            ),
        ],
    ),
];

/// Install default agent folders into `$WORKSPACE/agents/`, always
/// overwriting with the binary's built-in versions. Also installs
/// the default `crontab.lua`.
pub fn install_default_agents(workspace: &Path) -> Result<(), std::io::Error> {
    let agents_dir = workspace.join("agents");
    std::fs::create_dir_all(&agents_dir)?;

    for (name, files) in DEFAULT_AGENTS {
        let agent_dir = agents_dir.join(name);
        std::fs::create_dir_all(&agent_dir)?;
        for (filename, content) in *files {
            std::fs::write(agent_dir.join(filename), content)?;
        }
    }

    // Install LuaLS type stubs for agent developers
    let types_dir = agents_dir.join(".types");
    std::fs::create_dir_all(&types_dir)?;
    std::fs::write(types_dir.join("ghost.lua"), LUA_TYPE_STUBS)?;

    super::crontab::install_default_crontab(workspace)?;

    Ok(())
}

/// Validate a single agent's Lua config. Returns errors as strings.
pub fn validate_agent(workspace: &Path, name: &str) -> Vec<String> {
    let mut errors = Vec::new();

    let config = match load_agent(workspace, name) {
        Ok(c) => c,
        Err(e) => {
            errors.push(format!("Failed to load: {e}"));
            return errors;
        }
    };

    // Required fields
    if config.name.is_empty() {
        errors.push("Missing required field: name".to_string());
    }
    if config.description.is_empty() {
        errors.push("Missing required field: description".to_string());
    }

    // Warn if build hook is missing
    if !config.has_build {
        errors.push(
            "Missing build(ctx, args) function — agents must define a build hook".to_string(),
        );
    }

    // Validate tool names exist
    let known_tools = [
        "run_shell_command",
        "read_file",
        "write_file",
        "file_edit",
        "todo",
        "knowledge_search",
        "web_search",
        "web_fetch",
        "note_write",
        "agent_control",
    ];
    for tool in &config.tools {
        if !known_tools.contains(&tool.as_str()) {
            errors.push(format!("Unknown tool: {tool}"));
        }
    }

    // Validate custom tool JSON schemas
    for tool in &config.custom_tools {
        if tool.description.is_empty() {
            errors.push(format!("Custom tool '{}': missing description", tool.name));
        }
        if !tool.parameters.is_object() {
            errors.push(format!(
                "Custom tool '{}': parameters must be a JSON object",
                tool.name
            ));
        }
    }

    errors
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_lua_agent(workspace: &Path, name: &str, lua_content: &str) {
        let agent_dir = workspace.join("agents").join(name);
        std::fs::create_dir_all(&agent_dir).unwrap();
        std::fs::write(agent_dir.join("agent.lua"), lua_content).unwrap();
    }

    #[test]
    fn discover_agents_finds_lua_folders() {
        let dir = tempfile::TempDir::new().unwrap();
        setup_lua_agent(
            dir.path(),
            "alpha",
            r#"return { name = "alpha", description = "A agent", tools = {} }"#,
        );
        setup_lua_agent(
            dir.path(),
            "zeta",
            r#"return { name = "zeta", description = "Z agent", tools = {} }"#,
        );

        let found = discover_agents(dir.path());
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].name, "alpha");
        assert_eq!(found[1].name, "zeta");
    }

    #[test]
    fn discover_agents_skips_invalid() {
        let dir = tempfile::TempDir::new().unwrap();
        setup_lua_agent(
            dir.path(),
            "good",
            r#"return { name = "good", description = "Works", tools = {} }"#,
        );
        setup_lua_agent(dir.path(), "bad", "syntax error here!!!");

        let found = discover_agents(dir.path());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "good");
    }

    #[test]
    fn load_agent_not_found() {
        let dir = tempfile::TempDir::new().unwrap();
        let err = load_agent(dir.path(), "nonexistent").unwrap_err();
        assert!(matches!(err, AgentError::NotFound { .. }));
    }

    #[test]
    fn load_agent_with_host_returns_both() {
        let dir = tempfile::TempDir::new().unwrap();
        setup_lua_agent(
            dir.path(),
            "test",
            r#"return {
                name = "test",
                description = "Test agent",
                tools = { "todo" },
                pre_turn = function(state) return nil end,
            }"#,
        );

        let (config, _host) = load_agent_with_host(dir.path(), "test").unwrap();
        assert_eq!(config.name, "test");
        assert!(config.has_pre_turn);
    }

    #[test]
    fn validate_catches_missing_fields() {
        let dir = tempfile::TempDir::new().unwrap();
        setup_lua_agent(
            dir.path(),
            "empty",
            r#"return { name = "", description = "", tools = {} }"#,
        );

        let errors = validate_agent(dir.path(), "empty");
        assert!(errors.iter().any(|e| e.contains("name")));
        assert!(errors.iter().any(|e| e.contains("description")));
        assert!(errors.iter().any(|e| e.contains("build")));
    }

    #[test]
    fn validate_catches_unknown_tools() {
        let dir = tempfile::TempDir::new().unwrap();
        setup_lua_agent(
            dir.path(),
            "bad-tools",
            r#"return { name = "test", description = "Test", tools = { "nonexistent_tool" } }"#,
        );

        let errors = validate_agent(dir.path(), "bad-tools");
        assert!(
            errors
                .iter()
                .any(|e| e.contains("Unknown tool: nonexistent_tool"))
        );
    }

    #[test]
    fn validate_passes_for_valid_agent() {
        let dir = tempfile::TempDir::new().unwrap();
        setup_lua_agent(
            dir.path(),
            "valid",
            r#"return {
                name = "valid",
                description = "A valid agent",
                tools = { "web_search", "todo" },
                build = function(ctx, args)
                    return {
                        system_prompt = "test",
                        messages = {{ role = "user", content = args.prompt or "go" }},
                    }
                end,
            }"#,
        );

        let errors = validate_agent(dir.path(), "valid");
        assert!(errors.is_empty(), "Expected no errors, got: {errors:?}");
    }
}
