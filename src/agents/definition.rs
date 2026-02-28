use std::path::Path;

use serde::Deserialize;

use crate::providers::types::ReasoningEffort;

use super::error::TaskError;
use super::nudges::{
    ContextPressureConfig, IterationCountdownRule, ProgressGateConfig, RecencyConfig,
    TemporalConfig,
};

const DELIMITER: &str = "---";

/// A tool-count progress rule declared in agent YAML frontmatter.
///
/// Tracks how many times a tool has been called and optionally nudges the
/// model when below a minimum.
///
/// | `min` | `nudge` | Behavior                                     |
/// | ----- | ------- | -------------------------------------------- |
/// | set   | set     | Count shown; nudge printed while count < min |
/// | unset | set     | Count shown; nudge always printed            |
/// | set   | unset   | Count shown; nothing else                    |
/// | unset | unset   | Count shown; nothing else                    |
#[derive(Debug, Clone, Deserialize)]
pub struct ToolCountRule {
    pub tool: String,
    #[serde(default)]
    pub min: Option<u32>,
    #[serde(default)]
    pub nudge: Option<String>,
}

/// A progress rule declared in agent YAML frontmatter.
///
/// Can be either a tool-count rule (has `tool` field) or an iteration
/// countdown rule (has `remaining_iterations` field). Uses serde's
/// untagged enum to distinguish the two shapes.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum ProgressRule {
    IterationCountdown(IterationCountdownRule),
    ToolCount(ToolCountRule),
}

#[derive(Debug, Clone, Deserialize)]
struct TaskFrontmatter {
    name: String,
    description: String,
    tools: Vec<String>,
    #[serde(default = "default_max_iterations")]
    max_iterations: usize,
    model: Option<String>,
    #[serde(default)]
    progress: Vec<ProgressRule>,
    #[serde(default)]
    skills: Vec<String>,
    reasoning_effort: Option<ReasoningEffort>,
    progress_gate: Option<ProgressGateConfig>,
    temporal: Option<TemporalConfig>,
    recency: Option<RecencyConfig>,
    context_pressure: Option<ContextPressureConfig>,
}

fn default_max_iterations() -> usize {
    50
}

#[derive(Debug, Clone)]
pub struct TaskDefinition {
    pub name: String,
    pub description: String,
    pub tools: Vec<String>,
    pub max_iterations: usize,
    pub model: Option<String>,
    pub reasoning_effort: Option<ReasoningEffort>,
    pub progress_rules: Vec<ProgressRule>,
    pub skills: Vec<String>,
    pub system_prompt_template: String,
    pub progress_gate: Option<ProgressGateConfig>,
    pub temporal: Option<TemporalConfig>,
    pub recency: Option<RecencyConfig>,
    pub context_pressure: Option<ContextPressureConfig>,
}

impl TaskDefinition {
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

/// Parse an agent definition from a markdown file with YAML frontmatter.
///
/// Format:
/// ```text
/// ---
/// name: deep-research
/// description: "..."
/// tools:
///   - web_search
///   - web_fetch
/// max_iterations: 40
/// ---
///
/// # System prompt body here
/// {{ query }}
/// ```
pub fn parse_task_file(content: &str) -> Result<TaskDefinition, TaskError> {
    let trimmed = content.trim_start();

    if !trimmed.starts_with(DELIMITER) {
        return Err(TaskError::InvalidFrontMatter {
            reason: "missing opening --- delimiter".to_string(),
        });
    }

    let after_open = &trimmed[DELIMITER.len()..];
    let Some(end_pos) = after_open.find(DELIMITER) else {
        return Err(TaskError::InvalidFrontMatter {
            reason: "missing closing --- delimiter".to_string(),
        });
    };

    let frontmatter_str = &after_open[..end_pos];
    let body_start = DELIMITER.len() + end_pos + DELIMITER.len();
    let body = trimmed[body_start..]
        .strip_prefix('\n')
        .unwrap_or(&trimmed[body_start..])
        .to_string();

    let front: TaskFrontmatter = serde_yaml::from_str(frontmatter_str)
        .map_err(|source| TaskError::FrontMatterParse { source })?;

    Ok(TaskDefinition {
        name: front.name,
        description: front.description,
        tools: front.tools,
        max_iterations: front.max_iterations,
        model: front.model,
        reasoning_effort: front.reasoning_effort,
        progress_rules: front.progress,
        skills: front.skills,
        system_prompt_template: body,
        progress_gate: front.progress_gate,
        temporal: front.temporal,
        recency: front.recency,
        context_pressure: front.context_pressure,
    })
}

const DEFAULT_TASKS: &[(&str, &str)] = &[
    (
        "chat-reflection",
        include_str!("../../prompts/agents/chat-reflection.md"),
    ),
    (
        "deep-research",
        include_str!("../../prompts/agents/deep-research.md"),
    ),
];

/// Install default agent definitions into `$WORKSPACE/agents/`, always
/// overwriting with the binary's built-in versions.
pub fn install_default_tasks(workspace: &Path) -> Result<(), std::io::Error> {
    let agents_dir = workspace.join("agents");
    std::fs::create_dir_all(&agents_dir)?;

    for (name, content) in DEFAULT_TASKS {
        std::fs::write(agents_dir.join(format!("{name}.md")), content)?;
    }

    Ok(())
}

/// Minimal metadata for listing agents in the system prompt.
#[derive(Debug, Clone)]
pub struct TaskInfo {
    pub name: String,
    pub description: String,
}

/// Scan `$WORKSPACE/agents/` for agent definition files and return a sorted
/// list of agent name + description pairs.
#[tracing::instrument(skip_all, level = "debug", fields(workspace = %workspace.display()))]
pub fn discover_tasks(workspace: &Path) -> Vec<TaskInfo> {
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

        match parse_task_file(&content) {
            Ok(def) => agents.push(TaskInfo {
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
pub fn load_task(workspace: &Path, name: &str) -> Result<TaskDefinition, TaskError> {
    let path = workspace.join("agents").join(format!("{name}.md"));

    let content = std::fs::read_to_string(&path).map_err(|_| TaskError::NotFound {
        name: name.to_string(),
    })?;

    parse_task_file(&content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_agent_all_fields() {
        let content = "---\nname: deep-research\ndescription: Iterative web research\ntools:\n  - web_search\n  - web_fetch\n  - knowledge_search\nmax_iterations: 30\nmodel: fast\n---\n\n# Deep Research Agent\n\nYou are a research specialist.\n\nQuery: {{ query }}\n";
        let def = parse_task_file(content).unwrap();
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
        let content = "---\nname: simple\ndescription: A simple agent\ntools:\n  - todo\n---\n\nDo the thing. {{ query }}\n";
        let def = parse_task_file(content).unwrap();
        assert_eq!(def.max_iterations, 50);
        assert!(def.model.is_none());
        assert!(def.reasoning_effort.is_none());
    }

    #[test]
    fn parse_agent_with_reasoning_effort() {
        let content = "---\nname: low-effort\ndescription: Low reasoning\ntools:\n  - todo\nreasoning_effort: low\n---\n\nBody.\n";
        let def = parse_task_file(content).unwrap();
        assert_eq!(def.reasoning_effort, Some(ReasoningEffort::Low));
    }

    #[test]
    fn interpolate_query_and_date() {
        let content = "---\nname: test\ndescription: test\ntools: []\n---\n\nToday is {{ date }}. Research: {{ query }}\n\nAlso: {{query}}\n";
        let def = parse_task_file(content).unwrap();
        let rendered = def.render_system_prompt("3D printers under $1000");
        assert!(rendered.contains("Research: 3D printers under $1000"));
        assert!(rendered.contains("Also: 3D printers under $1000"));
        // Date should be interpolated as YYYY-MM-DD
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        assert!(rendered.contains(&today));
    }

    #[test]
    fn parse_missing_opening_delimiter() {
        let err = parse_task_file("name: oops").unwrap_err();
        assert!(err.to_string().contains("missing opening"));
    }

    #[test]
    fn parse_missing_closing_delimiter() {
        let err = parse_task_file("---\nname: oops").unwrap_err();
        assert!(err.to_string().contains("missing closing"));
    }

    #[test]
    fn parse_missing_required_field() {
        let content = "---\nname: x\n---\nBody\n";
        let err = parse_task_file(content).unwrap_err();
        assert!(matches!(err, TaskError::FrontMatterParse { .. }));
    }

    #[test]
    fn load_task_not_found() {
        let err = load_task(Path::new("/nonexistent"), "nope").unwrap_err();
        assert!(matches!(err, TaskError::NotFound { .. }));
    }

    #[test]
    fn install_default_tasks_creates_files() {
        let dir = tempfile::TempDir::new().unwrap();
        install_default_tasks(dir.path()).unwrap();

        for (name, _) in DEFAULT_TASKS {
            let agent_file = dir.path().join("agents").join(format!("{name}.md"));
            assert!(agent_file.exists(), "expected {agent_file:?} to exist");

            let def = load_task(dir.path(), name).unwrap();
            assert_eq!(def.name, *name);
        }
    }

    #[test]
    fn install_default_tasks_overwrites_existing() {
        let dir = tempfile::TempDir::new().unwrap();
        install_default_tasks(dir.path()).unwrap();

        let agent_file = dir
            .path()
            .join("agents")
            .join(format!("{}.md", DEFAULT_TASKS[0].0));
        std::fs::write(&agent_file, "custom content").unwrap();

        install_default_tasks(dir.path()).unwrap();

        let content = std::fs::read_to_string(&agent_file).unwrap();
        assert_ne!(content, "custom content", "should overwrite existing files");
    }

    #[test]
    fn discover_tasks_finds_and_sorts() {
        let dir = tempfile::TempDir::new().unwrap();
        let agents_dir = dir.path().join("agents");
        std::fs::create_dir_all(&agents_dir).unwrap();

        std::fs::write(
            agents_dir.join("zeta.md"),
            "---\nname: zeta\ndescription: Z agent\ntools: []\n---\nBody\n",
        )
        .unwrap();
        std::fs::write(
            agents_dir.join("alpha.md"),
            "---\nname: alpha\ndescription: A agent\ntools: []\n---\nBody\n",
        )
        .unwrap();

        let found = discover_tasks(dir.path());
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].name, "alpha");
        assert_eq!(found[0].description, "A agent");
        assert_eq!(found[1].name, "zeta");
    }

    #[test]
    fn discover_tasks_skips_non_md_and_invalid() {
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
            "---\nname: valid\ndescription: works\ntools: []\n---\nBody\n",
        )
        .unwrap();

        let found = discover_tasks(dir.path());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "valid");
    }

    #[test]
    fn default_deep_research_agent_parses() {
        let content = include_str!("../../prompts/agents/deep-research.md");
        let def = parse_task_file(content).unwrap();
        assert_eq!(def.name, "deep-research");
        assert!(def.tools.contains(&"web_search".to_string()));
        assert!(def.tools.contains(&"web_fetch".to_string()));
        assert!(def.tools.contains(&"todo".to_string()));
        assert_eq!(def.max_iterations, 30);
        assert!(def.system_prompt_template.contains("{{ date }}"));

        // Iteration countdown rules
        let countdown_rules: Vec<_> = def
            .progress_rules
            .iter()
            .filter_map(|r| match r {
                ProgressRule::IterationCountdown(ic) => Some(ic),
                _ => None,
            })
            .collect();
        assert_eq!(
            countdown_rules.len(),
            3,
            "deep-research has 3 countdown rules"
        );
        assert_eq!(countdown_rules[0].remaining_iterations, 10);
        assert_eq!(countdown_rules[1].remaining_iterations, 5);
        assert_eq!(countdown_rules[2].remaining_iterations, 2);

        // No tool-count rules
        let tool_count_rules: Vec<_> = def
            .progress_rules
            .iter()
            .filter_map(|r| match r {
                ProgressRule::ToolCount(tc) => Some(tc),
                _ => None,
            })
            .collect();
        assert!(tool_count_rules.is_empty());

        // Nudge configs
        let gate = def.progress_gate.as_ref().expect("progress_gate");
        assert!(gate.no_todo.contains("REJECTED"));
        assert!(gate.incomplete.contains("{incomplete}"));

        let temporal = def.temporal.as_ref().expect("temporal");
        assert_eq!(temporal.after_seconds, 300);
        assert!(temporal.message.iter().any(|m| m.contains("{minutes}")));

        assert_eq!(
            def.reasoning_effort,
            Some(ReasoningEffort::High),
            "deep-research should have high reasoning effort"
        );
        assert!(def.recency.is_none(), "recency nudge removed");
        let pressure = def
            .context_pressure
            .expect("deep-research should have context_pressure");
        assert!(
            (pressure.threshold_pct - 0.80).abs() < f64::EPSILON,
            "context_pressure threshold should be 80%"
        );
    }

    #[test]
    fn default_chat_reflection_agent_parses() {
        let content = include_str!("../../prompts/agents/chat-reflection.md");
        let def = parse_task_file(content).unwrap();
        assert_eq!(def.name, "chat-reflection");
        assert!(def.tools.contains(&"note_write".to_string()));
        assert!(def.tools.contains(&"write_file".to_string()));
        assert!(def.system_prompt_template.contains("{{ date }}"));
        assert!(def.progress_rules.is_empty());
        assert_eq!(def.skills, vec!["knowledge-navigator", "note-writer"]);
    }

    #[test]
    fn parse_agent_with_progress_rules() {
        let content = "---\nname: test-agent\ndescription: Test agent with progress\ntools:\n  - web_fetch\nprogress:\n  - tool: web_fetch\n    min: 3\n    nudge: \"Need {min} {tool} calls (have {count}). Keep going.\"\n---\n\nBody here.\n";
        let def = parse_task_file(content).unwrap();
        assert_eq!(def.progress_rules.len(), 1);
        let ProgressRule::ToolCount(rule) = &def.progress_rules[0] else {
            panic!("expected ToolCount variant");
        };
        assert_eq!(rule.tool, "web_fetch");
        assert_eq!(rule.min, Some(3));
        assert_eq!(
            rule.nudge.as_deref(),
            Some("Need {min} {tool} calls (have {count}). Keep going.")
        );
    }

    #[test]
    fn parse_agent_with_multiple_progress_rules() {
        let content = "---\nname: multi\ndescription: Multiple rules\ntools:\n  - web_fetch\n  - web_search\nprogress:\n  - tool: web_fetch\n    min: 5\n  - tool: web_search\n    min: 3\n    nudge: Search more.\n---\n\nBody.\n";
        let def = parse_task_file(content).unwrap();
        assert_eq!(def.progress_rules.len(), 2);
        let ProgressRule::ToolCount(r0) = &def.progress_rules[0] else {
            panic!("expected ToolCount");
        };
        assert_eq!(r0.tool, "web_fetch");
        assert_eq!(r0.min, Some(5));
        assert!(r0.nudge.is_none());
        let ProgressRule::ToolCount(r1) = &def.progress_rules[1] else {
            panic!("expected ToolCount");
        };
        assert_eq!(r1.tool, "web_search");
        assert_eq!(r1.min, Some(3));
        assert_eq!(r1.nudge.as_deref(), Some("Search more."));
    }

    #[test]
    fn parse_agent_without_progress_rules() {
        let content =
            "---\nname: simple\ndescription: No progress rules\ntools:\n  - todo\n---\n\nBody.\n";
        let def = parse_task_file(content).unwrap();
        assert!(def.progress_rules.is_empty());
        assert!(def.skills.is_empty());
        assert!(def.progress_gate.is_none());
        assert!(def.temporal.is_none());
        assert!(def.recency.is_none());
        assert!(def.context_pressure.is_none());
    }

    #[test]
    fn parse_agent_with_all_nudge_sections() {
        let content = "---\nname: nudgy\ndescription: Agent with all nudges\ntools:\n  - web_fetch\n  - todo\nprogress:\n  - tool: web_fetch\n    min: 5\n  - remaining_iterations: 10\n    message: \"{remaining} left.\"\nprogress_gate:\n  no_todo: Make a plan first.\n  incomplete: \"{incomplete} items left.\"\ntemporal:\n  after_seconds: 120\n  message: \"Been working {minutes} min.\"\nrecency:\n  tool: web_fetch\n  window: 2\n  message: Fetch something.\ncontext_pressure:\n  threshold_pct: 0.7\n  message: Context large.\n---\n\nBody.\n";
        let def = parse_task_file(content).unwrap();

        // Mixed progress rules: one tool-count + one iteration countdown
        assert_eq!(def.progress_rules.len(), 2);
        assert!(
            matches!(&def.progress_rules[0], ProgressRule::ToolCount(r) if r.tool == "web_fetch")
        );
        assert!(
            matches!(&def.progress_rules[1], ProgressRule::IterationCountdown(r) if r.remaining_iterations == 10)
        );

        let gate = def.progress_gate.unwrap();
        assert_eq!(gate.no_todo, "Make a plan first.");
        assert_eq!(gate.incomplete, "{incomplete} items left.");

        let temporal = def.temporal.unwrap();
        assert_eq!(temporal.after_seconds, 120);
        assert_eq!(temporal.message, vec!["Been working {minutes} min."]);

        let recency = def.recency.unwrap();
        assert_eq!(recency.tool, "web_fetch");
        assert_eq!(recency.window, 2);
        assert_eq!(recency.message, "Fetch something.");

        let pressure = def.context_pressure.unwrap();
        assert_eq!(pressure.threshold_pct, 0.7);
        assert_eq!(pressure.message, "Context large.");
    }

    #[test]
    fn parse_agent_with_iteration_countdown_rules() {
        let content = "---\nname: countdown\ndescription: Countdown agent\ntools:\n  - todo\nprogress:\n  - remaining_iterations: 10\n    message: \"{remaining} iterations left.\"\n  - remaining_iterations: 5\n    message: \"Only {remaining} left!\"\n---\n\nBody.\n";
        let def = parse_task_file(content).unwrap();
        assert_eq!(def.progress_rules.len(), 2);
        let ProgressRule::IterationCountdown(r0) = &def.progress_rules[0] else {
            panic!("expected IterationCountdown");
        };
        assert_eq!(r0.remaining_iterations, 10);
        assert_eq!(r0.message, "{remaining} iterations left.");
        let ProgressRule::IterationCountdown(r1) = &def.progress_rules[1] else {
            panic!("expected IterationCountdown");
        };
        assert_eq!(r1.remaining_iterations, 5);
        assert_eq!(r1.message, "Only {remaining} left!");
    }

    #[test]
    fn parse_agent_with_skills() {
        let content = "---\nname: skilled\ndescription: Agent with skills\ntools:\n  - knowledge_search\n  - note_write\nskills:\n  - knowledge-navigator\n  - note-writer\n---\n\nBody.\n";
        let def = parse_task_file(content).unwrap();
        assert_eq!(def.skills, vec!["knowledge-navigator", "note-writer"]);
    }
}
