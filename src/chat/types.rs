use serde::{Deserialize, Serialize};

use crate::config;
use crate::db::DatabaseError;
use crate::providers::{ProviderError, ProviderInitError};

pub const DEFAULT_MAX_TOOL_ITERATIONS: usize = 25;

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
pub enum ChatStopReason {
    EndTurn,
    MaxTokens,
    MaxIterations,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ChatResult {
    pub message: String,
    pub stop_reason: ChatStopReason,
}

#[derive(Debug, thiserror::Error)]
pub enum ChatError {
    #[error(transparent)]
    Database(Box<DatabaseError>),

    #[error(transparent)]
    Config(#[from] config::ConfigError),

    #[error(transparent)]
    Provider(#[from] ProviderError),

    #[error(transparent)]
    ProviderInit(#[from] ProviderInitError),

    #[error(transparent)]
    Prompt(#[from] crate::prompt::PromptError),

    #[error("invalid session id '{session_id}'")]
    InvalidSessionId { session_id: String },
}

impl From<DatabaseError> for ChatError {
    fn from(e: DatabaseError) -> Self {
        ChatError::Database(Box::new(e))
    }
}
