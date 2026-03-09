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
