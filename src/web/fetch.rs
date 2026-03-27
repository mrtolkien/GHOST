use std::sync::OnceLock;

use reqwest::header::CONTENT_TYPE;
use url::Url;

use super::{ExtractedContent, FetchOptions, WebError};

use crate::constants::MAX_EXTRACT_CHARS;

const HTTP_FETCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
const HEAD_REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

static HTTP_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

pub(super) fn client() -> &'static reqwest::Client {
    HTTP_CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(HTTP_FETCH_TIMEOUT)
            .user_agent("Mozilla/5.0 (compatible; Ghost/0.1)")
            .build()
            .expect("failed to build HTTP client")
    })
}

/// Fetch the raw HTML body of a URL. Returns `(html_body, final_url)`.
///
/// Used by the BFS crawler to get raw HTML for link extraction before
/// converting to markdown. Follows redirects; `final_url` reflects the
/// actual destination.
pub(crate) async fn fetch_raw(url: &str) -> Result<(String, String), WebError> {
    let parsed = Url::parse(url).map_err(|_| WebError::InvalidUrl {
        url: url.to_string(),
    })?;

    let response = client().get(parsed).send().await?;
    let final_url = response.url().to_string();

    if !response.status().is_success() {
        return Err(WebError::HttpStatus {
            status: response.status().as_u16(),
            url: url.to_string(),
        });
    }

    let bytes = response.bytes().await?;
    let body = String::from_utf8_lossy(&bytes).to_string();
    Ok((body, final_url))
}

/// Fetch a URL and extract readable content as markdown.
///
/// When `crawl4ai_url` is set, uses a HEAD request for cheap content-type
/// routing: HTML goes through crawl4ai (browser rendering), non-HTML text
/// through reqwest, and binary types are rejected. When crawl4ai is
/// unavailable, falls back to local extraction (htmd + readability).
#[tracing::instrument(name = "fetch url", skip_all, fields(url = %url))]
pub async fn fetch(
    url: &str,
    options: &FetchOptions,
    crawl4ai_url: Option<&str>,
    chrome_cdp_url: Option<&str>,
) -> Result<ExtractedContent, WebError> {
    let parsed = Url::parse(url).map_err(|_| WebError::InvalidUrl {
        url: url.to_string(),
    })?;

    match parsed.scheme() {
        "http" | "https" => {}
        _ => {
            return Err(WebError::InvalidUrl {
                url: url.to_string(),
            });
        }
    }

    let c4ai_options = super::crawl4ai::Crawl4aiOptions {
        wait_for: options.wait_for.clone(),
        css_selector: options.css_selector.clone(),
        scan_full_page: options.scan_full_page,
    };

    // When crawl4ai is available, use HEAD for cheap content-type routing.
    if let Some(c4ai_url) = crawl4ai_url {
        match head_content_type(&parsed).await {
            Ok(ct) => {
                if is_html_content_type(&ct) {
                    return fetch_html_via_crawl4ai(
                        c4ai_url,
                        url,
                        &c4ai_options,
                        options,
                        chrome_cdp_url,
                    )
                    .await;
                } else if is_text_content(&ct) {
                    return fetch_text_via_reqwest(url).await;
                } else {
                    return Err(WebError::UnsupportedContentType { content_type: ct });
                }
            }
            Err(_) => {
                // HEAD failed (403, 405, timeout) — try crawl4ai directly.
                tracing::info!(
                    url = url.to_string(),
                    "HEAD request failed, trying crawl4ai directly",
                );
                return fetch_html_via_crawl4ai(
                    c4ai_url,
                    url,
                    &c4ai_options,
                    options,
                    chrome_cdp_url,
                )
                .await;
            }
        }
    }

    // Legacy path: no crawl4ai — reqwest GET + local extraction.
    fetch_legacy(url, options).await
}

/// Cheap HEAD request — returns content-type string or error.
async fn head_content_type(url: &Url) -> Result<String, WebError> {
    let response = client()
        .head(url.clone())
        .timeout(HEAD_REQUEST_TIMEOUT)
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(WebError::HttpStatus {
            status: response.status().as_u16(),
            url: url.to_string(),
        });
    }
    Ok(parse_content_type(response.headers()).unwrap_or_default())
}

fn is_html_content_type(ct: &str) -> bool {
    ct == "text/html" || ct == "application/xhtml+xml" || ct.is_empty()
}

/// HTML path: crawl4ai first, local extraction fallback.
async fn fetch_html_via_crawl4ai(
    c4ai_url: &str,
    page_url: &str,
    c4ai_options: &super::crawl4ai::Crawl4aiOptions,
    fetch_options: &FetchOptions,
    cdp_url: Option<&str>,
) -> Result<ExtractedContent, WebError> {
    match super::crawl4ai::fetch_with_crawl4ai(c4ai_url, page_url, c4ai_options, cdp_url).await {
        Ok(markdown) => Ok(markdown_to_content(markdown, None)),
        Err(e) => {
            tracing::warn!(
                url = page_url.to_string(),
                error = e.to_string(),
                "crawl4ai failed, falling back to local extraction",
            );
            let (html, _final_url) = fetch_raw(page_url).await?;
            Ok(extract_content(&html, page_url, fetch_options))
        }
    }
}

/// Non-HTML text path: reqwest GET, return raw text.
async fn fetch_text_via_reqwest(url: &str) -> Result<ExtractedContent, WebError> {
    let parsed = Url::parse(url).map_err(|_| WebError::InvalidUrl {
        url: url.to_string(),
    })?;
    let response = client().get(parsed).send().await?;
    if !response.status().is_success() {
        return Err(WebError::HttpStatus {
            status: response.status().as_u16(),
            url: url.to_string(),
        });
    }
    let bytes = response.bytes().await?;
    let text = String::from_utf8_lossy(&bytes).replace('\0', "");
    let (text, truncated) = truncate(text, MAX_EXTRACT_CHARS);
    let word_count = text.split_whitespace().count();
    Ok(ExtractedContent {
        title: None,
        text,
        word_count,
        truncated,
    })
}

/// Legacy path: reqwest GET + local extraction (no crawl4ai).
/// Used by offline mode (passes crawl4ai_url=None).
async fn fetch_legacy(url: &str, options: &FetchOptions) -> Result<ExtractedContent, WebError> {
    let parsed = Url::parse(url).map_err(|_| WebError::InvalidUrl {
        url: url.to_string(),
    })?;

    let response = client().get(parsed).send().await?;
    let status = response.status().as_u16();

    if !response.status().is_success() {
        return Err(WebError::HttpStatus {
            status,
            url: url.to_string(),
        });
    }

    let content_type = parse_content_type(response.headers());

    if let Some(ref ct) = content_type
        && !is_text_content(ct)
    {
        return Err(WebError::UnsupportedContentType {
            content_type: ct.clone(),
        });
    }

    tracing::info!(
        url = url.to_string(),
        status = status as u64,
        content_type = content_type.as_deref().unwrap_or("unknown").to_string(),
        "web fetch complete (legacy)",
    );

    let bytes = response.bytes().await?;
    let raw_text = String::from_utf8_lossy(&bytes).to_string();

    let is_html = matches!(
        content_type.as_deref(),
        Some("text/html") | Some("application/xhtml+xml")
    );

    if is_html {
        Ok(extract_content(&raw_text, url, options))
    } else {
        let text = raw_text.replace('\0', "");
        let (text, truncated) = truncate(text, MAX_EXTRACT_CHARS);
        let word_count = text.split_whitespace().count();
        Ok(ExtractedContent {
            title: None,
            text,
            word_count,
            truncated,
        })
    }
}

fn markdown_to_content(markdown: String, title: Option<String>) -> ExtractedContent {
    let text = markdown.replace('\0', "");
    let (text, truncated) = truncate(text, MAX_EXTRACT_CHARS);
    let word_count = text.split_whitespace().count();
    ExtractedContent {
        title,
        text,
        word_count,
        truncated,
    }
}

/// Convert raw HTML to readable markdown content. Used by the BFS crawler
/// to avoid double-fetching pages.
pub(crate) fn extract_content(
    html: &str,
    page_url: &str,
    options: &FetchOptions,
) -> ExtractedContent {
    if options.raw {
        let text = html.replace('\0', "");
        let (text, truncated) = truncate(text, MAX_EXTRACT_CHARS);
        let word_count = text.split_whitespace().count();
        return ExtractedContent {
            title: None,
            text,
            word_count,
            truncated,
        };
    }

    let (title, text) = if options.readability {
        extract_with_readability(html, page_url)
    } else {
        let md = html_to_markdown(html);
        // Auto-retry with readability when htmd output is oversized.
        if md.len() > MAX_EXTRACT_CHARS {
            tracing::info!(
                htmd_len = md.len() as u64,
                "htmd output oversized, retrying with readability",
            );
            let (title, text) = extract_with_readability(html, page_url);
            // If readability actually reduced the size, use it.
            if text.len() < md.len() {
                (title, text)
            } else {
                (None, md)
            }
        } else {
            (None, md)
        }
    };

    let text = text.replace('\0', "");
    let (text, truncated) = truncate(text, MAX_EXTRACT_CHARS);
    let word_count = text.split_whitespace().count();

    ExtractedContent {
        title,
        text,
        word_count,
        truncated,
    }
}

fn extract_with_readability(html: &str, page_url: &str) -> (Option<String>, String) {
    let mut readability = match dom_smoothie::Readability::new(html, Some(page_url), None) {
        Ok(r) => r,
        Err(_) => return (None, html_to_markdown(html)),
    };

    match readability.parse() {
        Ok(article) if !article.content.trim().is_empty() => {
            let title = if article.title.trim().is_empty() {
                None
            } else {
                Some(article.title.to_string())
            };
            (title, html_to_markdown(&article.content))
        }
        _ => (None, html_to_markdown(html)),
    }
}

fn html_to_markdown(html: &str) -> String {
    static CONVERTER: OnceLock<htmd::HtmlToMarkdown> = OnceLock::new();
    let converter = CONVERTER.get_or_init(|| {
        htmd::HtmlToMarkdown::builder()
            .skip_tags(vec![
                "script", "style", "nav", "footer", "header", "noscript", "svg", "iframe",
            ])
            .build()
    });
    converter.convert(html).unwrap_or_else(|_| html.to_string())
}

fn parse_content_type(headers: &reqwest::header::HeaderMap) -> Option<String> {
    headers
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.split(';').next().unwrap_or(v).trim().to_string())
}

fn is_text_content(content_type: &str) -> bool {
    content_type.starts_with("text/")
        || content_type == "application/json"
        || content_type == "application/xml"
        || content_type == "application/xhtml+xml"
        || content_type == "application/rss+xml"
        || content_type == "application/atom+xml"
}

fn truncate(text: String, max_chars: usize) -> (String, bool) {
    let mut chars = text.chars();
    let truncated_text: String = chars.by_ref().take(max_chars).collect();
    let was_truncated = chars.next().is_some();
    (truncated_text, was_truncated)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_converts_to_markdown() {
        let html = r#"
        <html><body>
            <h1>Page Title</h1>
            <p>Article content here</p>
            <nav><a href="/about">About</a></nav>
        </body></html>"#;

        let options = FetchOptions::default();
        let result = extract_content(html, "http://example.com", &options);
        assert!(result.text.contains("Page Title"));
        assert!(result.text.contains("Article content"));
        assert!(
            !result.text.contains("About"),
            "nav content should be stripped"
        );
        assert!(result.title.is_none());
    }

    #[test]
    fn readability_mode_extracts_article() {
        let html = r#"
        <html><head><title>Test Article</title></head>
        <body>
            <article>
                <h1>Test Article</h1>
                <p>This is the main content of the article with enough text
                to be recognized by readability as meaningful content that
                should be extracted from the page.</p>
                <p>Another paragraph with more substantial content to ensure
                the readability algorithm has enough material to work with
                and produce a valid extraction result.</p>
            </article>
            <nav>Navigation links that should be stripped</nav>
        </body></html>"#;

        let options = FetchOptions {
            readability: true,
            ..Default::default()
        };
        let result = extract_content(html, "http://example.com/article", &options);
        assert!(result.text.contains("main content"));
        assert!(result.word_count > 0);
        assert!(!result.truncated);
    }

    #[test]
    fn readability_fallback_to_markdown() {
        let html = "<p>Simple paragraph</p>";
        let options = FetchOptions {
            readability: true,
            ..Default::default()
        };
        let result = extract_content(html, "http://example.com", &options);
        assert!(result.text.contains("Simple paragraph"));
    }

    #[test]
    fn raw_mode_returns_html() {
        let html = "<h1>Title</h1><p>Content</p>";
        let options = FetchOptions {
            raw: true,
            ..Default::default()
        };
        let result = extract_content(html, "http://example.com", &options);
        assert!(result.text.contains("<h1>Title</h1>"));
        assert!(result.text.contains("<p>Content</p>"));
    }

    #[test]
    fn truncation_at_internal_limit() {
        // The internal safety cap truncates extremely long content
        let (text, was_truncated) =
            truncate("a".repeat(MAX_EXTRACT_CHARS + 100), MAX_EXTRACT_CHARS);
        assert_eq!(text.len(), MAX_EXTRACT_CHARS);
        assert!(was_truncated);
    }

    #[test]
    fn null_bytes_removed() {
        let html = "<p>Hello\0World</p>";
        let options = FetchOptions::default();
        let result = extract_content(html, "http://example.com", &options);
        assert!(!result.text.contains('\0'));
        assert!(result.text.contains("Hello"));
    }

    #[test]
    fn text_content_type_whitelist() {
        assert!(is_text_content("text/html"));
        assert!(is_text_content("text/plain"));
        assert!(is_text_content("application/json"));
        assert!(is_text_content("application/xml"));
        assert!(is_text_content("application/xhtml+xml"));
        assert!(is_text_content("application/rss+xml"));
        assert!(is_text_content("application/atom+xml"));
        assert!(!is_text_content("image/png"));
        assert!(!is_text_content("application/pdf"));
        assert!(!is_text_content("application/octet-stream"));
    }

    #[test]
    fn truncate_short_text_unchanged() {
        let (text, was_truncated) = truncate("hello".to_string(), 100);
        assert_eq!(text, "hello");
        assert!(!was_truncated);
    }

    #[test]
    fn truncate_exact_length() {
        let (text, was_truncated) = truncate("hello".to_string(), 5);
        assert_eq!(text, "hello");
        assert!(!was_truncated);
    }

    #[test]
    fn auto_readability_on_oversized_htmd() {
        // Build HTML where htmd output exceeds MAX_EXTRACT_CHARS.
        // Use repeated paragraphs in an <article> — this is the dominant
        // content, so readability should extract it and produce a smaller
        // result than the raw htmd of the full page.
        // Scale repeats so the test works regardless of MAX_EXTRACT_CHARS value.
        let repeats = MAX_EXTRACT_CHARS / 80 + 100;
        let article_paragraphs = "<p>This is important article content that \
            discusses 3D printer reviews in detail with specs and benchmarks. \
            </p>"
            .repeat(repeats);
        let sidebar = "<div class=\"ad\">Buy now! Special offer!</div>".repeat(repeats / 5);
        let comments = "<p>User comment filler text. </p>".repeat(repeats / 5);
        let html = format!(
            r#"<html><head><title>Test Article</title></head><body>
            <div id="sidebar">{sidebar}</div>
            <article><h1>Test Article</h1>{article_paragraphs}</article>
            <div id="comments">{comments}</div>
            </body></html>"#
        );

        let options = FetchOptions::default();
        let plain = html_to_markdown(&html);
        assert!(
            plain.len() > MAX_EXTRACT_CHARS,
            "htmd output should exceed limit for this test: {} <= {}",
            plain.len(),
            MAX_EXTRACT_CHARS
        );

        let result = extract_content(&html, "http://example.com/article", &options);
        // Result should be within the limit regardless of whether readability
        // succeeded or we fell back to truncated htmd.
        assert!(
            result.text.len() <= MAX_EXTRACT_CHARS,
            "result should be within limit: {}",
            result.text.len()
        );
        // The article content should survive (it's either extracted by
        // readability or at least partially present in truncated htmd).
        assert!(
            result.text.contains("article content") || result.text.contains("3D printer"),
            "article content should be preserved"
        );
    }

    #[test]
    fn skip_tags_strips_junk_content() {
        let html = r#"
        <html><body>
            <h1>Title</h1>
            <p>Real content</p>
            <script>var x = 1;</script>
            <style>.foo { color: red; }</style>
            <footer>Copyright 2026</footer>
            <header><a href="/">Home</a></header>
            <svg><path d="M0 0"/></svg>
        </body></html>"#;

        let md = html_to_markdown(html);
        assert!(md.contains("Title"));
        assert!(md.contains("Real content"));
        assert!(!md.contains("var x"), "script content should be stripped");
        assert!(
            !md.contains("color: red"),
            "style content should be stripped"
        );
        assert!(
            !md.contains("Copyright"),
            "footer content should be stripped"
        );
        assert!(!md.contains("Home"), "header content should be stripped");
    }
}
