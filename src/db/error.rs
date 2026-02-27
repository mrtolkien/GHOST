use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum DatabaseError {
    #[error("failed to connect to SQLite at {path}: {source}")]
    Connect {
        path: PathBuf,
        #[source]
        source: sqlx::Error,
    },

    #[error("failed to run migrations: {source}")]
    Migrate {
        #[source]
        source: sqlx::migrate::MigrateError,
    },

    #[error("database query failed for table '{table}' operation '{operation}': {source}")]
    Query {
        table: &'static str,
        operation: &'static str,
        #[source]
        source: sqlx::Error,
    },

    #[error("query returned no rows for table '{table}' operation '{operation}'")]
    MissingRow {
        table: &'static str,
        operation: &'static str,
    },
}
