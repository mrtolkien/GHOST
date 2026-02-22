use std::path::Path;

use serde::Deserialize;

use super::error::AgentError;

const DELIMITER: &str = "+++";

/// A progress rule declared in agent TOML frontmatter.
///
/// Agents can declare minimum tool call counts with custom feedback
/// messages. The runtime injects these as `[Progress]` system messages
/// after each tool iteration.
#[derive(Debug, Clone, Deserialize)]
pub struct ProgressRule {
    pub tool: String,
    pub min: u32,
    #[serde(default)]
    pub below: Option<String>,
    #[serde(default)]
    pub met: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct AgentFrontmatter {
    name: String,
    description: String,
    tools: Vec<String>,
    #[serde(default = "default_max_iterations")]
    max_iterations: usize,
    model: Option<String>,
    #[serde(default)]
    progress: Vec<ProgressRule>,
}

fn default_max_iterations() -> usize {
    50
}

#[derive(Debug, Clone)]
pub struct AgentDefinition {
    pub name: String,
    pub description: String,
    pub tools: Vec<String>,
    pub max_iterations: usize,
    pub model: Option<String>,
    pub progress_rules: Vec<ProgressRule>,
    pub system_prompt_template: String,
}

impl AgentDefinition {
    /// Interpolate `{{ query }}` and `{{ date }}` in the system prompt.
    #[must_use]
    pub fn render_system_prompt(&self, query: &str) -> String {
        let date = chrono::Local::now().format("%Y-%m-%d").to_string();
        self.system_prompt_template
            .replace("{{ query }}", query)
            .replace("{{query}}", query)
            .replace("{{ date }}", &date)
            .replace("{{date}}", &date)
    }
}

/// Parse an agent definition from a markdown file with TOML frontmatter.
///
/// Format:
/// ```text
/// +++
/// name = "deep-research"
/// description = "..."
/// tools = ["web_search", "web_fetch"]
/// max_iterations = 40
/// +++
///
/// # System prompt body here
/// {{ query }}
/// ```
pub fn parse_agent_file(content: &str) -> Result<AgentDefinition, AgentError> {
    let trimmed = content.trim_start();

    if !trimmed.starts_with(DELIMITER) {
        return Err(AgentError::InvalidFrontMatter {
            reason: "missing opening +++ delimiter".to_string(),
        });
    }

    let after_open = &trimmed[DELIMITER.len()..];
    let Some(end_pos) = after_open.find(DELIMITER) else {
        return Err(AgentError::InvalidFrontMatter {
            reason: "missing closing +++ delimiter".to_string(),
        });
    };

    let frontmatter_str = &after_open[..end_pos];
    let body_start = DELIMITER.len() + end_pos + DELIMITER.len();
    let body = trimmed[body_start..]
        .strip_prefix('\n')
        .unwrap_or(&trimmed[body_start..])
        .to_string();

    let front: AgentFrontmatter = toml::from_str(frontmatter_str)
        .map_err(|source| AgentError::FrontMatterParse { source })?;

    Ok(AgentDefinition {
        name: front.name,
        description: front.description,
        tools: front.tools,
        max_iterations: front.max_iterations,
        model: front.model,
        progress_rules: front.progress,
        system_prompt_template: body,
    })
}

const DEFAULT_AGENTS: &[(&str, &str)] = &[(
    "deep-research",
    include_str!("../../prompts/agents/deep-research.md"),
)];

/// Install default agent definitions into `$WORKSPACE/agents/` if they don't
/// already exist.
pub fn install_default_agents(workspace: &Path) -> Result<(), std::io::Error> {
    let agents_dir = workspace.join("agents");
    std::fs::create_dir_all(&agents_dir)?;

    for (name, content) in DEFAULT_AGENTS {
        let agent_file = agents_dir.join(format!("{name}.md"));
        if !agent_file.exists() {
            std::fs::write(&agent_file, content)?;
        }
    }

    Ok(())
}

/// Minimal metadata for listing agents in the system prompt.
#[derive(Debug, Clone)]
pub struct AgentInfo {
    pub name: String,
    pub description: String,
}

/// Scan `$WORKSPACE/agents/` for agent definition files and return a sorted
/// list of agent name + description pairs.
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
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }

        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        match parse_agent_file(&content) {
            Ok(def) => agents.push(AgentInfo {
                name: def.name,
                description: def.description,
            }),
            Err(e) => {
                logfire::warn!(
                    "Malformed agent definition in {path}: {error}",
                    path = path.display().to_string(),
                    error = e.to_string(),
                );
            }
        }
    }

    agents.sort_by(|a, b| a.name.cmp(&b.name));
    agents
}

/// Load an agent definition from the workspace agents directory.
#[tracing::instrument(skip_all, fields(agent_name = name))]
pub fn load_agent(workspace: &Path, name: &str) -> Result<AgentDefinition, AgentError> {
    let path = workspace.join("agents").join(format!("{name}.md"));

    let content = std::fs::read_to_string(&path).map_err(|_| AgentError::NotFound {
        name: name.to_string(),
    })?;

    parse_agent_file(&content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_agent_all_fields() {
        let content = r#"+++
name = "deep-research"
description = "Iterative web research"
tools = ["web_search", "web_fetch", "knowledge_search"]
max_iterations = 30
model = "fast"
+++

# Deep Research Agent

You are a research specialist.

Query: {{ query }}
"#;
        let def = parse_agent_file(content).unwrap();
        assert_eq!(def.name, "deep-research");
        assert_eq!(def.description, "Iterative web research");
        assert_eq!(
            def.tools,
            vec!["web_search", "web_fetch", "knowledge_search"]
        );
        assert_eq!(def.max_iterations, 30);
        assert_eq!(def.model.as_deref(), Some("fast"));
        assert!(def.system_prompt_template.contains("research specialist"));
    }

    #[test]
    fn parse_defaults() {
        let content = r#"+++
name = "simple"
description = "A simple agent"
tools = ["todo"]
+++

Do the thing. {{ query }}
"#;
        let def = parse_agent_file(content).unwrap();
        assert_eq!(def.max_iterations, 50);
        assert!(def.model.is_none());
    }

    #[test]
    fn interpolate_query_and_date() {
        let content = r#"+++
name = "test"
description = "test"
tools = []
+++

Today is {{ date }}. Research: {{ query }}

Also: {{query}}
"#;
        let def = parse_agent_file(content).unwrap();
        let rendered = def.render_system_prompt("3D printers under $1000");
        assert!(rendered.contains("Research: 3D printers under $1000"));
        assert!(rendered.contains("Also: 3D printers under $1000"));
        // Date should be interpolated as YYYY-MM-DD
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        assert!(rendered.contains(&today));
    }

    #[test]
    fn parse_missing_opening_delimiter() {
        let err = parse_agent_file("name = \"oops\"").unwrap_err();
        assert!(err.to_string().contains("missing opening"));
    }

    #[test]
    fn parse_missing_closing_delimiter() {
        let err = parse_agent_file("+++\nname = \"oops\"").unwrap_err();
        assert!(err.to_string().contains("missing closing"));
    }

    #[test]
    fn parse_missing_required_field() {
        let content = "+++\nname = \"x\"\n+++\nBody\n";
        let err = parse_agent_file(content).unwrap_err();
        assert!(matches!(err, AgentError::FrontMatterParse { .. }));
    }

    #[test]
    fn load_agent_not_found() {
        let err = load_agent(Path::new("/nonexistent"), "nope").unwrap_err();
        assert!(matches!(err, AgentError::NotFound { .. }));
    }

    #[test]
    fn install_default_agents_creates_files() {
        let dir = tempfile::TempDir::new().unwrap();
        install_default_agents(dir.path()).unwrap();

        for (name, _) in DEFAULT_AGENTS {
            let agent_file = dir.path().join("agents").join(format!("{name}.md"));
            assert!(agent_file.exists(), "expected {agent_file:?} to exist");

            let def = load_agent(dir.path(), name).unwrap();
            assert_eq!(def.name, *name);
        }
    }

    #[test]
    fn install_default_agents_does_not_overwrite() {
        let dir = tempfile::TempDir::new().unwrap();
        install_default_agents(dir.path()).unwrap();

        let agent_file = dir
            .path()
            .join("agents")
            .join(format!("{}.md", DEFAULT_AGENTS[0].0));
        std::fs::write(&agent_file, "custom content").unwrap();

        install_default_agents(dir.path()).unwrap();

        let content = std::fs::read_to_string(&agent_file).unwrap();
        assert_eq!(content, "custom content");
    }

    #[test]
    fn discover_agents_finds_and_sorts() {
        let dir = tempfile::TempDir::new().unwrap();
        let agents_dir = dir.path().join("agents");
        std::fs::create_dir_all(&agents_dir).unwrap();

        std::fs::write(
            agents_dir.join("zeta.md"),
            "+++\nname = \"zeta\"\ndescription = \"Z agent\"\ntools = []\n+++\nBody\n",
        )
        .unwrap();
        std::fs::write(
            agents_dir.join("alpha.md"),
            "+++\nname = \"alpha\"\ndescription = \"A agent\"\ntools = []\n+++\nBody\n",
        )
        .unwrap();

        let found = discover_agents(dir.path());
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].name, "alpha");
        assert_eq!(found[0].description, "A agent");
        assert_eq!(found[1].name, "zeta");
    }

    #[test]
    fn discover_agents_skips_non_md_and_invalid() {
        let dir = tempfile::TempDir::new().unwrap();
        let agents_dir = dir.path().join("agents");
        std::fs::create_dir_all(&agents_dir).unwrap();

        // Non-md file
        std::fs::write(agents_dir.join("readme.txt"), "not an agent").unwrap();
        // Invalid frontmatter
        std::fs::write(agents_dir.join("broken.md"), "no frontmatter here").unwrap();
        // Valid
        std::fs::write(
            agents_dir.join("valid.md"),
            "+++\nname = \"valid\"\ndescription = \"works\"\ntools = []\n+++\nBody\n",
        )
        .unwrap();

        let found = discover_agents(dir.path());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "valid");
    }

    #[test]
    fn default_deep_research_agent_parses() {
        let content = include_str!("../../prompts/agents/deep-research.md");
        let def = parse_agent_file(content).unwrap();
        assert_eq!(def.name, "deep-research");
        assert!(def.tools.contains(&"web_search".to_string()));
        assert!(def.tools.contains(&"web_fetch".to_string()));
        assert!(def.tools.contains(&"todo".to_string()));
        assert!(def.system_prompt_template.contains("{{ date }}"));
        assert!(
            !def.progress_rules.is_empty(),
            "deep-research should declare progress rules"
        );
        let rule = &def.progress_rules[0];
        assert_eq!(rule.tool, "web_fetch");
        assert_eq!(rule.min, 7);
        assert!(rule.below.is_some());
        assert!(rule.met.is_some());
    }

    #[test]
    fn parse_agent_with_progress_rules() {
        let content = r#"+++
name = "test-agent"
description = "Test agent with progress"
tools = ["web_fetch"]

[[progress]]
tool = "web_fetch"
min = 3
below = "Need {min} {tool} calls (have {count}). Keep going."
met = "Done! {count}/{min} reached."
+++

Body here.
"#;
        let def = parse_agent_file(content).unwrap();
        assert_eq!(def.progress_rules.len(), 1);
        let rule = &def.progress_rules[0];
        assert_eq!(rule.tool, "web_fetch");
        assert_eq!(rule.min, 3);
        assert_eq!(
            rule.below.as_deref(),
            Some("Need {min} {tool} calls (have {count}). Keep going.")
        );
        assert_eq!(rule.met.as_deref(), Some("Done! {count}/{min} reached."));
    }

    #[test]
    fn parse_agent_with_multiple_progress_rules() {
        let content = r#"+++
name = "multi"
description = "Multiple rules"
tools = ["web_fetch", "web_search"]

[[progress]]
tool = "web_fetch"
min = 5

[[progress]]
tool = "web_search"
min = 3
below = "Search more."
+++

Body.
"#;
        let def = parse_agent_file(content).unwrap();
        assert_eq!(def.progress_rules.len(), 2);
        assert_eq!(def.progress_rules[0].tool, "web_fetch");
        assert_eq!(def.progress_rules[0].min, 5);
        assert!(def.progress_rules[0].below.is_none());
        assert_eq!(def.progress_rules[1].tool, "web_search");
        assert_eq!(def.progress_rules[1].min, 3);
        assert_eq!(def.progress_rules[1].below.as_deref(), Some("Search more."));
    }

    #[test]
    fn parse_agent_without_progress_rules() {
        let content = r#"+++
name = "simple"
description = "No progress rules"
tools = ["todo"]
+++

Body.
"#;
        let def = parse_agent_file(content).unwrap();
        assert!(def.progress_rules.is_empty());
    }
}
