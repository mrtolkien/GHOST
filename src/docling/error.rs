use thiserror::Error;

#[derive(Debug, Error)]
pub enum DoclingError {
    #[error("docling conversion failed: {0}")]
    Conversion(String),

    #[error("docling conversion timed out after {seconds}s")]
    Timeout { seconds: u64 },

    #[error("docling task failed: {detail}")]
    TaskFailed { detail: String },

    #[error("failed to parse DoclingDocument JSON: {0}")]
    Parse(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("render page failed: {0}")]
    RenderPage(String),

    #[error("vision extraction failed: {0}")]
    VisionExtraction(String),
}
