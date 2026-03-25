mod document;
mod error;
mod markdown;
mod quality;

pub use document::DoclingDocument;
pub use error::DoclingError;
pub use markdown::generate_markdown;
pub use quality::{PageQuality, assess_pages};
