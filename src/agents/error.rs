use thiserror::Error;

#[derive(Debug, Error)]
pub enum TaskError {
    #[error("agent '{name}' not found")]
    NotFound { name: String },

    #[error("agent '{name}' is already running (agent_id: {agent_id})")]
    AlreadyRunning { name: String, agent_id: String },

    #[error("agent not found: {agent_id}")]
    AgentNotFound { agent_id: String },

    #[error("no agent session found for id: {agent_session_id}")]
    AgentSessionNotFound { agent_session_id: String },

    #[error("agent execution failed: {message}")]
    ExecutionFailed { message: String },

    #[error("agent script error in '{agent}': {message}")]
    ScriptError { agent: String, message: String },

    #[error(transparent)]
    Chat(#[from] crate::chat::ChatError),

    #[error(transparent)]
    ProviderInit(#[from] crate::providers::ProviderInitError),

    #[error(transparent)]
    Database(Box<crate::db::DatabaseError>),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

impl From<crate::db::DatabaseError> for TaskError {
    fn from(e: crate::db::DatabaseError) -> Self {
        TaskError::Database(Box::new(e))
    }
}
