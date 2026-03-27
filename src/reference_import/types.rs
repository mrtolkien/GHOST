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

#[derive(Debug)]
pub struct UpdateResult {
    pub created: usize,
    pub updated: usize,
    pub deleted: usize,
    pub orphaned: usize,
    pub unchanged: usize,
    pub old_version_ref: Option<String>,
    pub new_version_ref: Option<String>,
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

    #[error("config error: {0}")]
    Config(String),

    #[error("docling error: {0}")]
    Docling(#[from] crate::docling::DoclingError),
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

impl ImportConfigJson {
    /// Reconstruct an `ImportConfig` from the serialized snapshot.
    /// Only source types that support update (git, crawl) are accepted.
    pub fn to_import_config(&self, topic: &str) -> Result<ImportConfig, ImportError> {
        let source = match self.source_type.as_str() {
            "git" => ImportSource::Git {
                url: self.source_url.clone(),
                paths: self.paths.clone(),
                extensions: self.extensions.clone(),
                git_ref: self.git_ref.clone(),
            },
            "crawl" => ImportSource::Crawl {
                url: self.source_url.clone(),
                max_depth: self
                    .max_depth
                    .unwrap_or(crate::constants::DEFAULT_CRAWL_MAX_DEPTH),
                max_pages: self
                    .max_pages
                    .unwrap_or(crate::constants::DEFAULT_CRAWL_MAX_PAGES),
            },
            other => {
                return Err(ImportError::Config(format!(
                    "unsupported source_type for update: {other}"
                )));
            }
        };
        Ok(ImportConfig {
            source,
            topic: topic.to_string(),
        })
    }
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
