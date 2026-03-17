use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug)]
pub struct ImportConfig {
    pub source: ImportSource,
    pub topic: String, // hierarchical name, e.g. "dioxus/docs"
}

#[derive(Debug)]
pub enum ImportSource {
    Git {
        url: String,
        paths: Vec<String>,
        extensions: Vec<String>,
        git_ref: Option<String>,
    },
    Page {
        url: String,
        no_ocr: bool,
        page_range: Option<(u32, u32)>,
    },
    Crawl {
        url: String,
        max_depth: usize,
        max_pages: usize,
    },
    File {
        path: String,
        no_ocr: bool,
        page_range: Option<(u32, u32)>,
    },
}

#[derive(Debug)]
pub struct ImportResult {
    pub topic_id: String,
    pub batch_id: String,
    pub references_created: usize,
    pub references_skipped: usize,
}

#[derive(Debug, Error)]
pub enum ImportError {
    #[error("git operation failed: {0}")]
    Git(String),

    #[error("fetch failed: {0}")]
    Fetch(String),

    #[error("database error: {0}")]
    Database(#[from] crate::db::DatabaseError),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Serializable snapshot of an `ImportConfig` for storage in DB and TOML.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportConfigJson {
    pub source_type: String,
    pub source_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_ref: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub paths: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub extensions: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_depth: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_pages: Option<usize>,
}

impl From<&ImportConfig> for ImportConfigJson {
    fn from(config: &ImportConfig) -> Self {
        match &config.source {
            ImportSource::Git {
                url,
                paths,
                extensions,
                git_ref,
            } => ImportConfigJson {
                source_type: "git".into(),
                source_url: url.clone(),
                git_ref: git_ref.clone(),
                paths: paths.clone(),
                extensions: extensions.clone(),
                max_depth: None,
                max_pages: None,
            },
            ImportSource::Crawl {
                url,
                max_depth,
                max_pages,
            } => ImportConfigJson {
                source_type: "crawl".into(),
                source_url: url.clone(),
                git_ref: None,
                paths: vec![],
                extensions: vec![],
                max_depth: Some(*max_depth),
                max_pages: Some(*max_pages),
            },
            ImportSource::Page { url, .. } => ImportConfigJson {
                source_type: "page".into(),
                source_url: url.clone(),
                git_ref: None,
                paths: vec![],
                extensions: vec![],
                max_depth: None,
                max_pages: None,
            },
            ImportSource::File { path, .. } => ImportConfigJson {
                source_type: "file".into(),
                source_url: path.clone(),
                git_ref: None,
                paths: vec![],
                extensions: vec![],
                max_depth: None,
                max_pages: None,
            },
        }
    }
}
