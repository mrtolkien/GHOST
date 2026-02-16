//! TEMPORARY SCAFFOLDING
//! This prompt renderer is a minimal bridge to support spec 06 orchestration.
//! It is expected to be replaced by the full prompt stack in spec 08.
//! WARNING: This file currently contains implementation code. Move logic into
//! dedicated module files before real prompt feature development continues.

use std::fs;

use crate::config::Config;

const BASE_PROMPT: &str = "You are GHOST, a personal AI agent for your OPERATOR.";

#[derive(Debug, Clone)]
pub struct PromptContext {
    pub model: String,
    pub provider: String,
}

#[derive(Debug, Clone)]
pub struct PromptRenderer {
    config: Config,
}

impl PromptRenderer {
    #[must_use]
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    #[tracing::instrument(skip_all)]
    pub fn render_system_prompt(&self, context: &PromptContext) -> String {
        let boot = read_optional(self.config.workspace.join("BOOT.md"));
        let soul = read_optional(self.config.workspace.join("SOUL.md"));
        let operator = read_optional(self.config.workspace.join("OPERATOR.md"));

        format!(
            "{BASE_PROMPT}\n\nModel: {}\nProvider: {}\n\n{}\n\n{}\n\n{}",
            context.model, context.provider, boot, soul, operator
        )
    }
}

fn read_optional(path: std::path::PathBuf) -> String {
    fs::read_to_string(path).unwrap_or_default()
}
