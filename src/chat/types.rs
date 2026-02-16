use serde::{Deserialize, Serialize};

use crate::config;
use crate::db::DatabaseError;
use crate::providers::{ProviderError, ProviderInitError};

pub const DEFAULT_MAX_TOOL_ITERATIONS: usize = 25;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct Citation {
    pub source: String,
    pub url: Option<String>,
    pub context: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
pub enum ChatStopReason {
    EndTurn,
    MaxTokens,
    MaxIterations,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ChatResult {
    pub message: String,
    pub citations: Vec<Citation>,
    pub stop_reason: ChatStopReason,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JobTranscript {
    pub transcript: String,
    pub result: ChatResult,
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

    #[error("invalid session id '{session_id}'")]
    InvalidSessionId { session_id: String },

    #[error("failed to parse structured response json: {0}")]
    InvalidStructuredResponse(#[from] serde_json::Error),
}

impl From<DatabaseError> for ChatError {
    fn from(e: DatabaseError) -> Self {
        ChatError::Database(Box::new(e))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct StructuredResponse {
    pub message: String,
    #[serde(default)]
    pub citations: Vec<StructuredCitation>,
}

#[derive(Debug, Deserialize)]
pub(super) struct StructuredCitation {
    pub source: String,
    pub context: Option<String>,
}
