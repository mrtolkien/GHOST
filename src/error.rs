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
    Embedding(#[from] crate::embeddings::EmbeddingError),

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

    #[error(transparent)]
    Web(#[from] crate::web::WebError),

    #[error(transparent)]
    Import(#[from] crate::reference_import::ImportError),

    #[error(transparent)]
    Project(#[from] crate::projects::ProjectError),

    #[error(transparent)]
    Coding(#[from] crate::coding::session::CodingError),

    #[error(transparent)]
    Onboarding(#[from] crate::onboarding::OnboardingError),

    #[error(transparent)]
    ServiceRegistry(#[from] crate::services::ServiceRegistryError),

    #[error(transparent)]
    PidFile(#[from] crate::daemon::pid_file::PidFileError),

    #[error("{0}")]
    Other(String),
}

impl From<crate::db::DatabaseError> for GhostError {
    fn from(e: crate::db::DatabaseError) -> Self {
        GhostError::Database(Box::new(e))
    }
}

pub fn repair_hint(message: &str) -> Option<&'static str> {
    let lower = message.to_ascii_lowercase();
    if lower.contains("database disk image is malformed") || lower.contains("code: 11") {
        return Some(
            "Database corruption detected. Run `ghost db repair --dry-run` to build and verify a repaired candidate database.",
        );
    }
    None
}

#[cfg(test)]
mod tests {
    use super::repair_hint;

    #[test]
    fn repair_hint_detects_malformed_sqlite_errors() {
        let hint = repair_hint(
            "database query failed for table 'reference' operation 'search': error returned from database: (code: 11) database disk image is malformed",
        );
        assert!(hint.is_some());
    }

    #[test]
    fn repair_hint_ignores_unrelated_errors() {
        assert!(repair_hint("query returned no rows").is_none());
    }
}
