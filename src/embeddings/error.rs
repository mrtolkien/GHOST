use thiserror::Error;

#[derive(Debug, Error)]
pub enum EmbeddingError {
    #[error("embedding API returned {status}: {body}")]
    Api { status: u16, body: String },

    #[error("embedding request failed: {source}")]
    Request {
        #[source]
        source: reqwest::Error,
    },

    #[error("embedding response contained no vectors")]
    EmptyResponse,

    #[error("embedding dimension mismatch: expected {expected}, got {actual}")]
    DimensionMismatch { expected: usize, actual: usize },
}

impl From<reqwest::Error> for EmbeddingError {
    fn from(source: reqwest::Error) -> Self {
        Self::Request { source }
    }
}
