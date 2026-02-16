use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum DatabaseError {
    #[error("failed to connect to SurrealDB at {path}: {source}")]
    Connect {
        path: PathBuf,
        #[source]
        source: surrealdb::Error,
    },

    #[error("failed to select namespace '{namespace}' and database '{database}': {source}")]
    SelectNamespace {
        namespace: &'static str,
        database: &'static str,
        #[source]
        source: surrealdb::Error,
    },

    #[error("failed to apply schema: {source}")]
    ApplySchema {
        #[source]
        source: surrealdb::Error,
    },

    #[error("database query failed for table '{table}' operation '{operation}': {source}")]
    Query {
        table: &'static str,
        operation: &'static str,
        #[source]
        source: surrealdb::Error,
    },

    #[error("query returned no rows for table '{table}' operation '{operation}'")]
    MissingRow {
        table: &'static str,
        operation: &'static str,
    },
}
