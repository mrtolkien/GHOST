pub mod agent_runs;
pub mod agent_state;
pub mod coding_sessions;
mod connection;
pub mod embeddings;
mod error;
pub mod interface_sessions;
pub mod knowledge;
pub mod sessions;

pub use connection::{GhostDb, connect};
pub use error::DatabaseError;

pub(crate) fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

pub(crate) fn new_id() -> String {
    ulid::Ulid::new().to_string()
}
