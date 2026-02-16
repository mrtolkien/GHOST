use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum PromptError {
    #[error("failed to read identity file {path}: {source}")]
    ReadIdentityFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("unknown template variable '{name}'")]
    TemplateVariable { name: String },

    #[error("failed to read skills directory {path}: {source}")]
    ReadSkillsDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}
