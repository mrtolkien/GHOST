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

    #[error("file upload failed: {reason}")]
    UploadFailed { reason: String },

    #[error("CDP error: {message}")]
    CdpError { message: String },

    #[error("URL not allowed: {reason}")]
    UrlBlocked { reason: String },

    #[error("no browser is active — connect to a browser first")]
    NoBrowserActive,

    #[error("no tab is active — open a tab first")]
    NoTabActive,

    #[error("browser '{name}' not found")]
    BrowserNotFound { name: String },

    #[error("tab {id} not found")]
    TabNotFound { id: u32 },

    #[error("tab limit reached ({limit} tabs) — close a tab first")]
    TabLimitReached { limit: usize },

    #[error("browser '{name}' connection lost: {reason}. reconnect in progress")]
    ConnectionLost { name: String, reason: String },

    #[error("browser '{name}' reconnect exhausted after {attempts} attempts: {reason}")]
    ReconnectExhausted {
        name: String,
        attempts: usize,
        reason: String,
    },

    #[error("CDP discovery failed: {reason}")]
    DiscoveryFailed { reason: String },
}
