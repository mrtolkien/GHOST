use thiserror::Error;

#[derive(Debug, Error)]
pub enum BrowserError {
    #[error("browser not connected — is Chrome running at {url}?")]
    ConnectionFailed {
        url: String,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    #[error("navigation to {url} failed: {reason}")]
    NavigationFailed { url: String, reason: String },

    #[error("navigation to {url} timed out after {timeout_secs}s")]
    NavigationTimeout { url: String, timeout_secs: u64 },

    #[error("element [ref={ref_id}] not found — page may have changed, try 'snapshot'")]
    RefNotFound { ref_id: String },

    #[error("element [ref={ref_id}] is not interactable: {reason}")]
    NotInteractable { ref_id: String, reason: String },

    #[error("screenshot failed: {reason}")]
    ScreenshotFailed { reason: String },

    #[error("CDP error: {message}")]
    CdpError { message: String },

    #[error("URL not allowed: {reason}")]
    UrlBlocked { reason: String },
}
