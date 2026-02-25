mod connection;
pub mod embeddings;
mod error;
pub mod interface_sessions;
pub mod job_logs;
pub mod knowledge;
pub mod query;
pub mod schema;
pub mod sessions;

pub use connection::{GhostDb, connect};
pub use error::DatabaseError;

/// Format a `RecordId` as `table:key` for logging and display.
///
/// `RecordId` in surrealdb 3.x no longer implements `Display`.
/// This helper uses `ToSql` internally to produce the `table:key` format.
#[must_use]
pub fn fmt_id(id: &surrealdb::types::RecordId) -> String {
    use surrealdb::types::ToSql;
    id.to_sql()
}
