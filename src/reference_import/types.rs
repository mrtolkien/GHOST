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
    Book {
        path: String,
        title: Option<String>,
        authors: Vec<String>,
    },
}

#[derive(Debug)]
pub struct ImportResult {
    pub topic_id: String,
    pub batch_id: Option<String>,
    pub references_created: usize,
    pub references_skipped: usize,
}

/// Provenance metadata for an import — optional, passed through from convert step.
#[derive(Debug, Clone, Default)]
pub struct ImportProvenance {
    pub source_type: Option<String>,
    pub source_url: Option<String>,
    pub version_ref: Option<String>,
    pub git_ref: Option<String>,
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

impl From<crate::convert::error::ConvertError> for ImportError {
    fn from(e: crate::convert::error::ConvertError) -> Self {
        use crate::convert::error::ConvertError;
        match e {
            ConvertError::Git(s) => ImportError::Git(s),
            ConvertError::Fetch(s) => ImportError::Fetch(s),
            ConvertError::Conversion(s) => ImportError::Config(s),
            ConvertError::Io(io) => ImportError::Io(io),
        }
    }
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authors: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publisher: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publication_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub published_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_seconds: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcript_source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chapter_count: Option<usize>,
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
                title: None,
                authors: None,
                language: None,
                publisher: None,
                publication_date: None,
                video_id: None,
                channel: None,
                published_at: None,
                duration_seconds: None,
                transcript_source: None,
                section_count: None,
                chapter_count: None,
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
                title: None,
                authors: None,
                language: None,
                publisher: None,
                publication_date: None,
                video_id: None,
                channel: None,
                published_at: None,
                duration_seconds: None,
                transcript_source: None,
                section_count: None,
                chapter_count: None,
            },
            ImportSource::File { path, .. } => ImportConfigJson {
                source_type: "file".into(),
                source_url: path.clone(),
                git_ref: None,
                paths: vec![],
                extensions: vec![],
                max_depth: None,
                max_pages: None,
                title: None,
                authors: None,
                language: None,
                publisher: None,
                publication_date: None,
                video_id: None,
                channel: None,
                published_at: None,
                duration_seconds: None,
                transcript_source: None,
                section_count: None,
                chapter_count: None,
            },
            ImportSource::Book {
                path,
                title,
                authors,
            } => ImportConfigJson {
                source_type: "book".into(),
                source_url: path.clone(),
                git_ref: None,
                paths: vec![],
                extensions: vec![],
                max_depth: None,
                max_pages: None,
                title: title.clone(),
                authors: Some(authors.clone()),
                language: None,
                publisher: None,
                publication_date: None,
                video_id: None,
                channel: None,
                published_at: None,
                duration_seconds: None,
                transcript_source: None,
                section_count: None,
                chapter_count: None,
            },
        }
    }
}
