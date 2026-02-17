use thiserror::Error;

#[derive(Debug, Error)]
pub enum ToolError {
    #[error("tool '{name}' not found")]
    NotFound { name: String },

    #[error("invalid parameters: {0}")]
    InvalidParams(String),

    #[error("execution failed: {0}")]
    ExecutionFailed(String),

    #[error("permission denied: {0}")]
    PermissionDenied(String),
}
