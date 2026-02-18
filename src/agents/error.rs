use thiserror::Error;

#[derive(Debug, Error)]
pub enum AgentError {
    #[error("agent '{name}' not found")]
    NotFound { name: String },

    #[error("invalid agent frontmatter: {reason}")]
    InvalidFrontMatter { reason: String },

    #[error("failed to parse agent frontmatter: {source}")]
    FrontMatterParse {
        #[source]
        source: toml::de::Error,
    },

    #[error("agent '{name}' is already running (agent_id: {agent_id})")]
    AlreadyRunning { name: String, agent_id: String },

    #[error("agent not found: {agent_id}")]
    AgentNotFound { agent_id: String },

    #[error(transparent)]
    Chat(#[from] crate::chat::ChatError),

    #[error(transparent)]
    ProviderInit(#[from] crate::providers::ProviderInitError),

    #[error(transparent)]
    Database(Box<crate::db::DatabaseError>),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

impl From<crate::db::DatabaseError> for AgentError {
    fn from(e: crate::db::DatabaseError) -> Self {
        AgentError::Database(Box::new(e))
    }
}
