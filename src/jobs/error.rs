use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum JobError {
    #[error("invalid job frontmatter: {reason}")]
    InvalidFrontMatter { reason: String },

    #[error("failed to parse job frontmatter: {source}")]
    FrontMatterParse {
        #[source]
        source: toml::de::Error,
    },

    #[error("invalid cron schedule '{expression}': {reason}")]
    InvalidSchedule { expression: String, reason: String },

    #[error("job file not found: {path}")]
    FileNotFound { path: PathBuf },

    #[error("io error reading {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("job '{name}' not found")]
    JobNotFound { name: String },

    #[error(transparent)]
    Agent(#[from] crate::agents::TaskError),

    #[error(transparent)]
    Database(Box<crate::db::DatabaseError>),
}

impl From<crate::db::DatabaseError> for JobError {
    fn from(e: crate::db::DatabaseError) -> Self {
        JobError::Database(Box::new(e))
    }
}
