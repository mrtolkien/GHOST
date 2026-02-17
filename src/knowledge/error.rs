use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum KnowledgeError {
    #[error("invalid frontmatter: {reason}")]
    InvalidFrontMatter { reason: String },

    #[error("failed to parse frontmatter: {source}")]
    FrontMatterParse {
        #[source]
        source: toml::de::Error,
    },

    #[error("failed to serialize frontmatter: {source}")]
    FrontMatterSerialize {
        #[source]
        source: toml::ser::Error,
    },

    #[error("note not found: {title}")]
    NoteNotFound { title: String },

    #[error("file not found: {path}")]
    FileNotFound { path: PathBuf },

    #[error("io error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error(transparent)]
    Database(#[from] Box<crate::db::DatabaseError>),
}
