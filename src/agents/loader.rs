use std::path::{Path, PathBuf};

use crate::scripting::host::ScriptHost;
use crate::scripting::types::AgentConfig;

use super::error::AgentError;

/// Minimal metadata for listing agents in the system prompt.
#[derive(Debug, Clone)]
pub struct AgentInfo {
    pub name: String,
    pub description: String,
}

/// Resolve the directory containing `agent.lua` for a given agent name.
///
/// Search order:
/// 1. `$WORKSPACE/agents/{name}/agent.lua` (trigger/scheduled agents)
/// 2. `$WORKSPACE/skills/**//{name}/agent.lua` (skill-coupled agents)
///
/// Returns `None` if no matching agent is found.
pub fn resolve_agent_dir(workspace: &Path, name: &str) -> Option<PathBuf> {
    // Primary: agents directory
    let agents_path = workspace.join("agents").join(name);
    if agents_path.join("agent.lua").exists() {
        return Some(agents_path);
    }

    // Secondary: walk skills directory
    let skills_dir = workspace.join("skills");
    find_agent_in_dir(&skills_dir, name)
}

/// Recursively search a directory tree for a subdirectory named `name`
/// that contains `agent.lua`.
fn find_agent_in_dir(dir: &Path, name: &str) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let dir_name = entry.file_name();
        let dir_name = dir_name.to_string_lossy();
        if dir_name.starts_with('.') {
            continue;
        }

        if *dir_name == *name && path.join("agent.lua").exists() {
            return Some(path);
        }

        // Recurse into subdirectories
        if let Some(found) = find_agent_in_dir(&path, name) {
            return Some(found);
        }
    }
    None
}

/// Scan `$WORKSPACE/agents/` and `$WORKSPACE/skills/` for agent
/// definition folders and return a sorted, deduplicated list of agent
/// name + description pairs. `agents/` is scanned first so its entries
/// win on name collisions (matching `resolve_agent_dir` priority).
#[tracing::instrument(skip_all, level = "debug", fields(workspace = %workspace.display()))]
pub fn discover_agents(workspace: &Path) -> Vec<AgentInfo> {
    let mut agents = Vec::new();

    // Scan agents/ directory first (priority source)
    collect_agents_from_dir(&workspace.join("agents"), workspace, &mut agents);

    // Scan skills/ directory recursively
    collect_agents_recursive(&workspace.join("skills"), workspace, &mut agents);

    // Deduplicate by name — first-seen wins (agents/ entries came first)
    let mut seen = std::collections::HashSet::new();
    agents.retain(|a| {
        if seen.contains(&a.name) {
            tracing::warn!(
                name = a.name.as_str(),
                "Duplicate agent name — keeping agents/ version",
            );
            false
        } else {
            seen.insert(a.name.clone());
            true
        }
    });

    agents.sort_by(|a, b| a.name.cmp(&b.name));
    agents
}

/// Collect agents from a flat directory (each subdirectory with agent.lua).
fn collect_agents_from_dir(dir: &Path, workspace: &Path, agents: &mut Vec<AgentInfo>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        if path.join("agent.lua").exists() {
            try_load_agent_info(&path, workspace, agents);
        }
    }
}

/// Recursively collect agents from a directory tree.
fn collect_agents_recursive(dir: &Path, workspace: &Path, agents: &mut Vec<AgentInfo>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let dir_name = entry.file_name();
        if dir_name.to_string_lossy().starts_with('.') {
            continue;
        }

        if path.join("agent.lua").exists() {
            try_load_agent_info(&path, workspace, agents);
        } else {
            collect_agents_recursive(&path, workspace, agents);
        }
    }
}

fn try_load_agent_info(agent_dir: &Path, workspace: &Path, agents: &mut Vec<AgentInfo>) {
    match load_agent_from_dir(agent_dir, workspace) {
        Ok(config) => agents.push(AgentInfo {
            name: config.name,
            description: config.description,
        }),
        Err(e) => {
            tracing::warn!(
                path = agent_dir.display().to_string(),
                error = e.to_string(),
                "Malformed agent.lua",
            );
        }
    }
}

/// Load an agent's config from its Lua folder. Lightweight — drops the VM
/// after extracting the config.
#[tracing::instrument(skip_all, fields(agent_name = name), level="debug")]
pub fn load_agent(workspace: &Path, name: &str) -> Result<AgentConfig, AgentError> {
    let agent_dir = resolve_agent_dir(workspace, name).ok_or_else(|| AgentError::NotFound {
        name: name.to_string(),
    })?;
    load_agent_from_dir(&agent_dir, workspace)
}

/// Load an agent config directly from a known directory.
fn load_agent_from_dir(agent_dir: &Path, workspace: &Path) -> Result<AgentConfig, AgentError> {
    let name = agent_dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    let mut host = ScriptHost::new(agent_dir, workspace).map_err(|e| AgentError::ScriptError {
        agent: name.clone(),
        message: e.to_string(),
    })?;

    host.load_config().map_err(|e| AgentError::ScriptError {
        agent: name,
        message: e.to_string(),
    })
}

/// Load an agent's config and keep the ScriptHost alive for hook execution.
#[tracing::instrument(skip_all, fields(agent_name = name), level="debug")]
pub fn load_agent_with_host(
    workspace: &Path,
    name: &str,
) -> Result<(AgentConfig, ScriptHost), AgentError> {
    let agent_dir = resolve_agent_dir(workspace, name).ok_or_else(|| AgentError::NotFound {
        name: name.to_string(),
    })?;

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

/// Validate a single agent's Lua config. Returns errors as strings.
/// Resolves agent by name from both `agents/` and `skills/`.
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
        "shell",
        "file_read",
        "file_write",
        "file_edit",
        "todo",
        "knowledge_search",
        "web_search",
        "web_fetch",
        "note_write",
        "agent",
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

    fn setup_skill_agent(workspace: &Path, skill_path: &str, name: &str, lua_content: &str) {
        let agent_dir = workspace.join("skills").join(skill_path).join(name);
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
    fn discover_agents_finds_skill_coupled_agents() {
        let dir = tempfile::TempDir::new().unwrap();
        setup_skill_agent(
            dir.path(),
            "my-skill",
            "my-agent",
            r#"return { name = "my-agent", description = "Skill agent", tools = {} }"#,
        );

        let found = discover_agents(dir.path());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "my-agent");
    }

    #[test]
    fn discover_agents_finds_nested_skill_agents() {
        let dir = tempfile::TempDir::new().unwrap();
        setup_skill_agent(
            dir.path(),
            "superpowers/subagent-dev",
            "implementer",
            r#"return { name = "implementer", description = "Impl", tools = {} }"#,
        );

        let found = discover_agents(dir.path());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "implementer");
    }

    #[test]
    fn discover_agents_finds_both_agents_and_skill_agents() {
        let dir = tempfile::TempDir::new().unwrap();
        setup_lua_agent(
            dir.path(),
            "trigger-agent",
            r#"return { name = "trigger-agent", description = "Triggered", tools = {} }"#,
        );
        setup_skill_agent(
            dir.path(),
            "research",
            "researcher",
            r#"return { name = "researcher", description = "Researches", tools = {} }"#,
        );

        let found = discover_agents(dir.path());
        assert_eq!(found.len(), 2);
        let names: Vec<&str> = found.iter().map(|a| a.name.as_str()).collect();
        assert!(names.contains(&"trigger-agent"));
        assert!(names.contains(&"researcher"));
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
    fn resolve_agent_dir_prefers_agents_over_skills() {
        let dir = tempfile::TempDir::new().unwrap();
        setup_lua_agent(
            dir.path(),
            "my-agent",
            r#"return { name = "my-agent", description = "In agents/", tools = {} }"#,
        );
        setup_skill_agent(
            dir.path(),
            "some-skill",
            "my-agent",
            r#"return { name = "my-agent", description = "In skills/", tools = {} }"#,
        );

        let resolved = resolve_agent_dir(dir.path(), "my-agent").unwrap();
        assert_eq!(resolved, dir.path().join("agents").join("my-agent"));
    }

    #[test]
    fn resolve_agent_dir_finds_skill_agent() {
        let dir = tempfile::TempDir::new().unwrap();
        setup_skill_agent(
            dir.path(),
            "deep-research",
            "deep-research",
            r#"return { name = "deep-research", description = "Research", tools = {} }"#,
        );

        let resolved = resolve_agent_dir(dir.path(), "deep-research").unwrap();
        assert!(resolved.ends_with("skills/deep-research/deep-research"));
    }

    #[test]
    fn resolve_agent_dir_returns_none_for_missing() {
        let dir = tempfile::TempDir::new().unwrap();
        assert!(resolve_agent_dir(dir.path(), "nonexistent").is_none());
    }

    #[test]
    fn load_agent_not_found() {
        let dir = tempfile::TempDir::new().unwrap();
        let err = load_agent(dir.path(), "nonexistent").unwrap_err();
        assert!(matches!(err, AgentError::NotFound { .. }));
    }

    #[test]
    fn load_agent_from_skill_dir() {
        let dir = tempfile::TempDir::new().unwrap();
        setup_skill_agent(
            dir.path(),
            "my-skill",
            "skill-agent",
            r#"return { name = "skill-agent", description = "From skill", tools = {} }"#,
        );

        let config = load_agent(dir.path(), "skill-agent").unwrap();
        assert_eq!(config.name, "skill-agent");
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
