use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProjectError {
    #[error("invalid frontmatter: {reason}")]
    InvalidFrontMatter { reason: String },

    #[error("failed to parse frontmatter: {source}")]
    FrontMatterParse {
        #[source]
        source: serde_yaml::Error,
    },

    #[error("failed to serialize frontmatter: {source}")]
    FrontMatterSerialize {
        #[source]
        source: serde_yaml::Error,
    },

    #[error("project not found: {slug}")]
    ProjectNotFound { slug: String },

    #[error("task not found: {project}/{task}")]
    TaskNotFound { project: String, task: String },

    #[error("io error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}
