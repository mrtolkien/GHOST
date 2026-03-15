mod crawl4ai;
mod cache;
pub mod curation;
pub mod docling;
mod fetch;
mod search;
mod searxng;
mod types;
pub mod browser;

pub use crawl4ai::{Crawl4aiOptions, fetch_with_crawl4ai};
pub use cache::{
    format_search_metadata, save_fetch_cache, save_search_cache, scan_web_cache, slug_from_url,
};
pub use curation::{
    ClassifiedCacheFile, CurationResult, classify_web_cache, curate_references,
    format_classified_cache, link_cited_edges,
};
pub use fetch::fetch;
pub(crate) use fetch::{extract_content, fetch_raw};
pub use search::BraveSearchProvider;
pub use searxng::SearxngSearchProvider;
pub use types::{ExtractedContent, FetchOptions, SearchResult, WebError};
