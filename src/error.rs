#[derive(Debug, thiserror::Error)]
pub enum GhostError {
    #[error("{command} is not yet implemented")]
    NotYetImplemented { command: &'static str },

    #[error(transparent)]
    Config(#[from] crate::config::ConfigError),

    #[error(transparent)]
    Observability(#[from] crate::observability::ObservabilityError),

    #[error(transparent)]
    Database(Box<crate::db::DatabaseError>),

    #[error(transparent)]
    Auth(#[from] crate::auth::openai_oauth::AuthError),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Chat(#[from] crate::chat::ChatError),

    #[error(transparent)]
    Prompt(#[from] crate::prompt::PromptError),

    #[error(transparent)]
    Discord(#[from] crate::interfaces::discord::DiscordError),
}

impl From<crate::db::DatabaseError> for GhostError {
    fn from(e: crate::db::DatabaseError) -> Self {
        GhostError::Database(Box::new(e))
    }
}
