use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::UnboundedSender;

use crate::config;
use crate::db::DatabaseError;
use crate::providers::{ProviderError, ProviderInitError};
use crate::tools::TodoItem;

pub const DEFAULT_MAX_TOOL_ITERATIONS: usize = 25;

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
pub enum ChatStopReason {
    EndTurn,
    MaxTokens,
    MaxIterations,
    Stopped,
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

    #[error("session '{session_id}' already has an active tool loop")]
    SessionBusy { session_id: String },
}

/// Accumulated metadata from a complete tool loop run.
#[derive(Debug, Clone, Default)]
pub struct RunMetadata {
    pub model_alias: String,
    pub iterations: usize,
    pub tool_counts: HashMap<String, usize>,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cache_read_tokens: u32,
    pub duration: std::time::Duration,
}

/// A single tool call with a short argument summary.
#[derive(Debug, Clone)]
pub struct ToolCallInfo {
    pub name: String,
    pub args_summary: String,
    /// Human-readable display string from `tools::display`.
    pub display: String,
}

/// A single tool result with display strings for UI editing.
#[derive(Debug, Clone)]
pub struct ToolResultInfo {
    pub name: String,
    pub display_request: String,
    pub display_result: String,
}

/// Events emitted by the tool loop for live UI updates.
///
/// Interface-agnostic — rendering is handled by the receiver
/// (e.g. `DiscordUiRenderer`).
#[derive(Debug, Clone)]
pub enum ToolLoopEvent {
    ToolCalls { calls: Vec<ToolCallInfo> },
    ToolResults { results: Vec<ToolResultInfo> },
    TodoUpdated { items: Vec<TodoItem> },
    Compacted,
}

/// Convenience alias for the event sender.
pub type EventSender = UnboundedSender<ToolLoopEvent>;

impl From<DatabaseError> for ChatError {
    fn from(e: DatabaseError) -> Self {
        ChatError::Database(Box::new(e))
    }
}

impl ChatError {
    /// True when the error means the session ID doesn't exist in this
    /// instance's database — useful for multi-instance setups where only one
    /// instance owns the session.
    pub fn is_session_not_found(&self) -> bool {
        matches!(
            self,
            ChatError::Database(db)
                if matches!(**db, DatabaseError::MissingRow { table: "session", .. })
        )
    }
}
