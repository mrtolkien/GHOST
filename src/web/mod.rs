mod browser;
mod cache;
mod fetch;
mod search;
mod types;

pub use browser::fetch_with_crawl4ai;
pub use cache::{save_fetch_cache, save_search_cache, scan_web_cache};
pub use fetch::fetch;
pub use search::BraveSearchProvider;
pub use types::{ExtractedContent, FetchOptions, SearchResult, WebError};
