mod chunker;
mod client;
pub mod error;
pub mod pipeline;

pub use chunker::{Chunk, chunk_text};
pub use client::EmbeddingClient;
pub use error::EmbeddingError;
