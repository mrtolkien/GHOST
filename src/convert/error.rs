use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConvertError {
    #[error("git error: {0}")]
    Git(String),
    #[error("fetch error: {0}")]
    Fetch(String),
    #[error("conversion error: {0}")]
    Conversion(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
