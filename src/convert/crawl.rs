use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};

use scraper::{Html, Selector};
use url::Url;

use crate::web;

use super::error::ConvertError;
use super::staging::{create_staging_dir, slug_from_source};

/// Polite delay between sequential HTTP fetches during a crawl.
const CRAWL_DELAY_MS: u64 = 200;

/// Result of crawling a website into a staging directory.
#[derive(Debug)]
#[must_use]
pub struct CrawlConvertResult {
    /// Path to the staging directory containing the converted markdown files.
    pub staging_dir: PathBuf,
    /// The seed URL used to start the crawl.
    pub source_url: String,
    /// Map from relative filename (e.g. `"index.md"`) to the source URL for
    /// each crawled page, in crawl order.
    pub page_urls: Vec<(String, String)>,
}

/// BFS-crawl a website and write each page as a markdown file in a staging
/// directory.
///
/// Starts from `seed_url`, stays on the same host, and stops when
/// `max_depth` or `max_pages` is reached. Each HTML page is converted to
/// markdown via [`crate::web::extract_content`] and written as a `.md` file
/// named after the URL path.
#[tracing::instrument(
    name = "convert_crawl",
    skip_all,
    fields(seed_url = %seed_url, staging_root = %staging_root.display())
)]
pub async fn convert_crawl(
    staging_root: &Path,
    seed_url: &str,
    max_depth: usize,
    max_pages: usize,
) -> Result<CrawlConvertResult, ConvertError> {
    let seed =
        Url::parse(seed_url).map_err(|e| ConvertError::Fetch(format!("invalid seed URL: {e}")))?;
    let seed_host = seed
        .host_str()
        .ok_or_else(|| ConvertError::Fetch("seed URL has no host".into()))?
        .to_string();

    let slug = slug_from_source(seed_url);
    let staging_dir = create_staging_dir(staging_root, &slug)?;

    let mut queue: VecDeque<(Url, usize)> = VecDeque::new();
    let mut visited: HashSet<String> = HashSet::new();
    let mut page_urls: Vec<(String, String)> = Vec::new();

    queue.push_back((seed, 0));

    while let Some((url, depth)) = queue.pop_front() {
        if page_urls.len() >= max_pages {
            break;
        }

        let normalized = normalize_url(&url);
        if visited.contains(&normalized) || depth > max_depth {
            continue;
        }
        visited.insert(normalized.clone());

        let page_num = page_urls.len() + 1;
        tracing::debug!(url = url.as_str(), page = page_num, "crawl: fetching page");

        let (html, final_url) = match web::fetch_raw(url.as_str()).await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(
                    url = url.as_str(),
                    error = %e,
                    "crawl: failed to fetch page, skipping",
                );
                continue;
            }
        };

        // Extract same-host links and enqueue before converting
        if depth < max_depth {
            enqueue_links(
                &html, &final_url, &url, &seed_host, &visited, depth, &mut queue,
            );
        }

        // Convert HTML to markdown and write to staging
        let extracted = web::extract_content(&html, url.as_str(), &web::FetchOptions::default());
        let filename = url_to_filename(&url);
        let dest = staging_dir.join(&filename);

        std::fs::write(&dest, &extracted.text)?;
        page_urls.push((filename, url.to_string()));

        tokio::time::sleep(std::time::Duration::from_millis(CRAWL_DELAY_MS)).await;
    }

    Ok(CrawlConvertResult {
        staging_dir,
        source_url: seed_url.to_string(),
        page_urls,
    })
}

/// Enqueue all same-host links discovered in `html` that have not been visited.
fn enqueue_links(
    html: &str,
    final_url: &str,
    original_url: &Url,
    seed_host: &str,
    visited: &HashSet<String>,
    depth: usize,
    queue: &mut VecDeque<(Url, usize)>,
) {
    let Ok(link_selector) = Selector::parse("a[href]") else {
        return;
    };

    let base = Url::parse(final_url).unwrap_or_else(|_| original_url.clone());
    let doc = Html::parse_document(html);

    for element in doc.select(&link_selector) {
        let Some(href) = element.value().attr("href") else {
            continue;
        };
        let Ok(resolved) = base.join(href) else {
            continue;
        };
        if resolved.host_str() != Some(seed_host) {
            continue;
        }
        let norm = normalize_url(&resolved);
        if !visited.contains(&norm) {
            queue.push_back((resolved, depth + 1));
        }
    }
}

/// Normalize a URL by stripping fragment and query params for deduplication.
fn normalize_url(url: &Url) -> String {
    let mut normalized = url.clone();
    normalized.set_fragment(None);
    normalized.set_query(None);
    let s = normalized.to_string();
    s.strip_suffix('/').unwrap_or(&s).to_string()
}

/// Derive a markdown filename from a URL path.
///
/// The root path (`/`) maps to `index.md`. Other paths replace `/` with `-`
/// and trim leading/trailing separators (e.g. `/docs/getting-started` →
/// `docs-getting-started.md`).
fn url_to_filename(url: &Url) -> String {
    let path = url.path().trim_matches('/');
    if path.is_empty() {
        return "index.md".to_string();
    }
    let slug = path.replace('/', "-");
    format!("{slug}.md")
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- normalize_url ---

    #[test]
    fn normalize_strips_fragment_and_query() {
        let url = Url::parse("https://example.com/page?foo=bar#section").unwrap();
        assert_eq!(normalize_url(&url), "https://example.com/page");
    }

    #[test]
    fn normalize_strips_trailing_slash() {
        let url = Url::parse("https://example.com/page/").unwrap();
        assert_eq!(normalize_url(&url), "https://example.com/page");
    }

    #[test]
    fn normalize_preserves_path() {
        let url = Url::parse("https://example.com/docs/getting-started").unwrap();
        assert_eq!(
            normalize_url(&url),
            "https://example.com/docs/getting-started"
        );
    }

    #[test]
    fn normalize_root_url() {
        let url = Url::parse("https://example.com/").unwrap();
        assert_eq!(normalize_url(&url), "https://example.com");
    }

    // --- url_to_filename ---

    #[test]
    fn filename_root_is_index() {
        let url = Url::parse("https://example.com/").unwrap();
        assert_eq!(url_to_filename(&url), "index.md");
    }

    #[test]
    fn filename_single_segment() {
        let url = Url::parse("https://example.com/docs").unwrap();
        assert_eq!(url_to_filename(&url), "docs.md");
    }

    #[test]
    fn filename_nested_path() {
        let url = Url::parse("https://example.com/docs/getting-started").unwrap();
        assert_eq!(url_to_filename(&url), "docs-getting-started.md");
    }

    #[test]
    fn filename_deep_path() {
        let url = Url::parse("https://example.com/a/b/c/d").unwrap();
        assert_eq!(url_to_filename(&url), "a-b-c-d.md");
    }

    #[test]
    fn filename_trailing_slash_trimmed() {
        let url = Url::parse("https://example.com/docs/").unwrap();
        // path().trim_matches('/') removes both leading and trailing slashes
        assert_eq!(url_to_filename(&url), "docs.md");
    }
}
