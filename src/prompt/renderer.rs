use std::collections::HashMap;

use crate::config::Config;

use super::context;
use super::error::PromptError;
use super::template::render_template;

const BASE_SYSTEM_PROMPT: &str = include_str!("../../prompts/chat-system.md");

/// Input context for rendering a system prompt.
#[derive(Debug, Clone)]
pub struct PromptContext {
    pub model: String,
    pub provider: String,
}

/// Input context for rendering a job prompt.
#[derive(Debug, Clone)]
pub struct JobPromptContext {
    pub prompt_body: String,
    pub previous_handoff: Option<String>,
    pub diary_today: Option<String>,
    pub recent_messages: Option<String>,
    pub web_cache_files: Option<String>,
}

/// Assembles system and job prompts from templates and runtime context.
#[derive(Debug, Clone)]
pub struct PromptRenderer {
    config: Config,
}

impl PromptRenderer {
    #[must_use]
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    /// Render the full system prompt for a chat session.
    #[tracing::instrument(skip_all, level = "debug", fields(model = %context.model))]
    pub fn render_system_prompt(&self, context: &PromptContext) -> Result<String, PromptError> {
        let workspace = &self.config.workspace;

        let ghost_identity = context::build_ghost_identity(workspace);
        let operator_context = context::build_operator_context(workspace);
        let ghost_diary = context::build_ghost_diary(workspace);
        let ghost_skills = context::build_ghost_skills(workspace);
        let active_projects = context::build_active_projects(workspace);
        let system_info = context::build_system_info(workspace);
        let model_info = context::build_model_info(&context.model, &context.provider);

        let mut vars: HashMap<&str, String> = HashMap::new();
        vars.insert("ghost_identity", ghost_identity);
        vars.insert("operator_context", operator_context);
        vars.insert("ghost_diary", ghost_diary);
        vars.insert("ghost_skills", ghost_skills);
        vars.insert("active_projects", active_projects);
        vars.insert("system_info", system_info);
        vars.insert("model_info", model_info);

        Ok(render_template(BASE_SYSTEM_PROMPT, &vars))
    }

    /// Render a job prompt with job-specific variables.
    #[tracing::instrument(skip_all, level = "debug", fields(job_name = %job_name))]
    pub fn render_job_prompt(
        &self,
        job_name: &str,
        context: &JobPromptContext,
    ) -> Result<String, PromptError> {
        let _ = job_name; // used only in the tracing span

        let mut vars: HashMap<&str, String> = HashMap::new();
        if let Some(ref handoff) = context.previous_handoff {
            vars.insert("previous_handoff", handoff.clone());
        }
        if let Some(ref diary) = context.diary_today {
            vars.insert("diary_today", diary.clone());
        }
        if let Some(ref msgs) = context.recent_messages {
            vars.insert("recent_messages", msgs.clone());
        }
        if let Some(ref files) = context.web_cache_files {
            vars.insert("web_cache_files", files.clone());
        }

        Ok(render_template(&context.prompt_body, &vars))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::test_config;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn full_render_injects_identity_and_runtime_context() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("BOOT.md"), "# BOOT\nBe helpful.").unwrap();
        fs::write(dir.path().join("SOUL.md"), "I am Ghost.").unwrap();

        let renderer = PromptRenderer::new(test_config(dir.path()));
        let prompt = renderer
            .render_system_prompt(&PromptContext {
                model: "test-model".to_string(),
                provider: "test-provider".to_string(),
            })
            .unwrap();

        // Identity files interpolated
        assert!(prompt.contains("Be helpful."));
        assert!(prompt.contains("I am Ghost."));
        // Runtime context interpolated
        assert!(prompt.contains("Model: test-model"));
        assert!(prompt.contains("Provider: test-provider"));
        assert!(prompt.contains("OS:"));
        assert!(prompt.contains(&dir.path().display().to_string()));
    }

    #[test]
    fn render_succeeds_with_no_workspace_files() {
        let dir = TempDir::new().unwrap();

        let prompt = PromptRenderer::new(test_config(dir.path()))
            .render_system_prompt(&PromptContext {
                model: "m".to_string(),
                provider: "p".to_string(),
            })
            .unwrap();

        // Base template still present, runtime context still filled
        assert!(prompt.contains("System Prompt"));
        assert!(prompt.contains("Model: m"));
    }

    #[test]
    fn job_prompt_interpolates_provided_vars_and_blanks_missing() {
        let dir = TempDir::new().unwrap();
        let renderer = PromptRenderer::new(test_config(dir.path()));

        let with_vars = renderer
            .render_job_prompt(
                "reflection",
                &JobPromptContext {
                    prompt_body: "H:{{ previous_handoff }} D:{{ diary_today }}".to_string(),
                    previous_handoff: Some("all good".to_string()),
                    diary_today: Some("wrote code".to_string()),
                    recent_messages: None,
                    web_cache_files: None,
                },
            )
            .unwrap();
        assert_eq!(with_vars, "H:all good D:wrote code");

        let without_vars = renderer
            .render_job_prompt(
                "reflection",
                &JobPromptContext {
                    prompt_body: "prev={{ previous_handoff }}".to_string(),
                    previous_handoff: None,
                    diary_today: None,
                    recent_messages: None,
                    web_cache_files: None,
                },
            )
            .unwrap();
        assert_eq!(without_vars, "prev=");
    }
}
