mod chunker;
mod client;
mod code_chunker;
pub mod error;
pub mod pipeline;

pub use chunker::{Chunk, chunk_text};
pub use client::EmbeddingClient;
pub use code_chunker::chunk_code;
pub use error::EmbeddingError;
