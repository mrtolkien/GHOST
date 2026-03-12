use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::config::Config;
use crate::prompt::template::render_template;
use crate::skills;

const CODING_AGENT_PROMPT: &str = include_str!("../../prompts/coding-agent.md");

pub fn build_coding_prompt(config: &Config, working_dir: &Path) -> String {
    let repo_context = load_repo_context(working_dir);
    let coding_skills = build_coding_skills(&config.workspace, working_dir);
    let model_info = build_model_info(config);

    let mut vars: HashMap<&str, String> = HashMap::new();
    vars.insert("working_dir", working_dir.display().to_string());
    vars.insert("repo_context", repo_context);
    vars.insert("coding_skills", coding_skills);
    vars.insert("model_info", model_info);

    render_template(CODING_AGENT_PROMPT, &vars)
}

fn load_repo_context(working_dir: &Path) -> String {
    for name in ["AGENTS.md", "CLAUDE.md"] {
        let path = working_dir.join(name);
        if let Ok(content) = fs::read_to_string(&path) {
            return format!("## Project Conventions ({name})\n\n{content}");
        }
    }
    String::new()
}

fn build_coding_skills(workspace: &Path, working_dir: &Path) -> String {
    let mut all_skills = skills::discover_skills(workspace);

    // Also discover repo-local skills from .agents/skills/
    let repo_skills_dir = working_dir.join(".agents").join("skills");
    if repo_skills_dir.is_dir() {
        let repo_skills = discover_repo_skills(&repo_skills_dir);
        all_skills.extend(repo_skills);
    }

    if all_skills.is_empty() {
        return String::new();
    }

    let entries: Vec<String> = all_skills
        .iter()
        .map(|s| {
            let source_tag = s
                .source
                .as_ref()
                .map(|src| format!("\n    <source>{src}</source>"))
                .unwrap_or_default();
            format!(
                "  <skill>\n    <name>{}</name>\n    \
                 <description>{}</description>\n    \
                 <location>{}</location>{source_tag}\n  </skill>",
                s.name,
                s.description,
                s.path.display(),
            )
        })
        .collect();

    format!(
        "## Available Skills\n\n\
         <available_skills>\n{}\n</available_skills>",
        entries.join("\n"),
    )
}

fn discover_repo_skills(skills_dir: &Path) -> Vec<skills::Skill> {
    let entries = match fs::read_dir(skills_dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    let mut found = Vec::new();
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let skill_path = entry.path().join("skill.md");
        let content = match fs::read_to_string(&skill_path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        if let Some(fm) = skills::parse_frontmatter(&content) {
            found.push(skills::Skill {
                name: fm.name,
                description: fm.description,
                available: fm.available,
                source: fm.source,
                path: skill_path,
            });
        }
    }
    found.sort_by(|a, b| a.name.cmp(&b.name));
    found
}

fn build_model_info(config: &Config) -> String {
    let alias = config
        .coding
        .model
        .as_deref()
        .unwrap_or(&config.models.default);
    let model = config
        .models
        .aliases
        .get(alias)
        .map(|m| m.model.as_str())
        .unwrap_or("unknown");
    format!("Model: {model}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn build_coding_prompt_includes_working_dir() {
        let ws = TempDir::new().unwrap();
        let config = crate::config::test_config(ws.path());
        let dir = TempDir::new().unwrap();
        let prompt = build_coding_prompt(&config, dir.path());
        assert!(prompt.contains(&dir.path().display().to_string()));
    }

    #[test]
    fn load_repo_context_reads_agents_md() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("AGENTS.md"), "# Test\nBe nice.").unwrap();
        let ctx = load_repo_context(dir.path());
        assert!(ctx.contains("Be nice."));
        assert!(ctx.contains("AGENTS.md"));
    }

    #[test]
    fn load_repo_context_prefers_agents_over_claude() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("AGENTS.md"), "agents").unwrap();
        fs::write(dir.path().join("CLAUDE.md"), "claude").unwrap();
        let ctx = load_repo_context(dir.path());
        assert!(ctx.contains("agents"));
        assert!(!ctx.contains("claude"));
    }

    #[test]
    fn load_repo_context_empty_when_no_file() {
        let dir = TempDir::new().unwrap();
        let ctx = load_repo_context(dir.path());
        assert!(ctx.is_empty());
    }
}
