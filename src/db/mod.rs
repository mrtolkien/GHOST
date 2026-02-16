mod connection;
mod error;
pub mod interface_sessions;
pub mod job_logs;
pub mod knowledge;
pub mod schema;
pub mod sessions;

pub use connection::{GhostDb, connect};
pub use error::DatabaseError;
