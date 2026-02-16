#[derive(Debug, thiserror::Error)]
pub enum GhostError {
    #[error("{command} is not yet implemented")]
    NotYetImplemented { command: &'static str },

    #[error(transparent)]
    Config(#[from] crate::config::ConfigError),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
