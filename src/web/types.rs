use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum WebError {
    #[error("missing API key: {name} environment variable not set")]
    MissingApiKey { name: &'static str },

    #[error("HTTP request failed: {0}")]
    Request(#[from] reqwest::Error),

    #[error("search API error (HTTP {status}): {body}")]
    SearchApi { status: u16, body: String },

    #[error("invalid URL: {url}")]
    InvalidUrl { url: String },

    #[error("unsupported content type: {content_type}")]
    UnsupportedContentType { content_type: String },

    #[error("non-success status {status} fetching {url}")]
    HttpStatus { status: u16, url: String },

    #[error("failed to write cache file {path}: {source}")]
    CacheWrite {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to read cache directory {path}: {source}")]
    CacheRead {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("crawl4ai failed for {url}: {detail}")]
    Crawl4ai { url: String, detail: String },

    #[error("docling conversion failed: {0}")]
    Docling(String),
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: Option<String>,
    /// Which search engines found this result (SearXNG only).
    pub engines: Option<Vec<String>>,
    /// Position of this result in each engine's ranking (SearXNG only).
    pub positions: Option<Vec<u32>>,
    /// Aggregated relevance score (SearXNG only).
    pub score: Option<f64>,
    /// Publication date as returned by the search engine (SearXNG only).
    pub published_date: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ExtractedContent {
    pub title: Option<String>,
    pub text: String,
    pub word_count: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, Default)]
pub struct FetchOptions {
    pub readability: bool,
    pub raw: bool,
}
