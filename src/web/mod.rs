pub mod browser;
mod cache;
mod crawl4ai;
pub mod curation;
mod fetch;
mod search;
mod searxng;
mod types;
pub mod url_match;

pub use cache::{format_search_metadata, save_fetch_cache, save_search_cache, scan_web_cache};
pub use crawl4ai::{Crawl4aiOptions, fetch_with_crawl4ai};
pub use curation::{
    ClassifiedCacheFile, CurationResult, classify_web_cache, curate_references,
    format_classified_cache, link_cited_edges,
};
pub use fetch::fetch;
pub(crate) use fetch::{extract_content, fetch_raw};
pub use search::BraveSearchProvider;
pub use searxng::SearxngSearchProvider;
pub use types::{ExtractedContent, FetchOptions, SearchResult, WebError};
pub use url_match::slug_from_url;
