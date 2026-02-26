use std::sync::OnceLock;

use reqwest::header::CONTENT_TYPE;
use url::Url;

use super::{ExtractedContent, FetchOptions, WebError};

/// Default safety cap for extracted text. Pages exceeding this are truncated.
/// When htmd produces content above this limit, we auto-retry with readability
/// mode to strip boilerplate before truncating.
///
/// 30K chars ≈ 7.5K tokens — allows ~10 fetches before hitting context pressure.
/// Key content (recommendations, prices, brand mentions) clusters in the first
/// 30K of most review/article pages.
const MAX_EXTRACT_CHARS: usize = 30_000;

static HTTP_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

pub(super) fn client() -> &'static reqwest::Client {
    HTTP_CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .user_agent("Mozilla/5.0 (compatible; Ghost/0.1)")
            .build()
            .expect("failed to build HTTP client")
    })
}

/// Fetch a URL and extract readable content as markdown.
///
/// For HTML pages, converts to markdown (via htmd) with optional readability
/// extraction. Falls back to crawl4ai for JS-heavy pages or HTTP errors.
/// Non-HTML text is returned as-is. Content is truncated to 30K chars.
#[tracing::instrument(skip_all, fields(url = %url))]
pub async fn fetch(
    url: &str,
    options: &FetchOptions,
    crawl4ai_url: Option<&str>,
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

    let response = client().get(parsed.clone()).send().await?;
    let status = response.status().as_u16();

    if !response.status().is_success() {
        if let Some(c4ai_url) = crawl4ai_url {
            logfire::info!(
                "HTTP error, trying crawl4ai fallback",
                url = url.to_string(),
                status = status as u64,
            );
            match super::browser::fetch_with_crawl4ai(c4ai_url, url).await {
                Ok(markdown) => return Ok(markdown_to_content(markdown, None)),
                Err(e) => {
                    logfire::warn!(
                        "crawl4ai fallback also failed after HTTP error",
                        url = url.to_string(),
                        original_status = status as u64,
                        crawl4ai_error = e.to_string(),
                    );
                }
            }
        }
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

    logfire::info!(
        "web fetch complete",
        url = url.to_string(),
        status = status as u64,
        content_type = content_type.as_deref().unwrap_or("unknown").to_string(),
    );

    let bytes = response.bytes().await?;
    let raw_text = String::from_utf8_lossy(&bytes).to_string();

    let is_html = matches!(
        content_type.as_deref(),
        Some("text/html") | Some("application/xhtml+xml")
    );

    if is_html {
        let mut content = extract_content(&raw_text, url, options);

        if !options.raw
            && content.word_count < 500
            && let Some(c4ai_url) = crawl4ai_url
        {
            logfire::info!(
                "reqwest extraction yielded low content, trying crawl4ai",
                url = url.to_string(),
                word_count = content.word_count as u64,
            );

            match super::browser::fetch_with_crawl4ai(c4ai_url, url).await {
                Ok(markdown) => {
                    content = markdown_to_content(markdown, content.title);
                }
                Err(e) => {
                    logfire::warn!(
                        "crawl4ai fallback failed, returning reqwest result",
                        url = url.to_string(),
                        error = e.to_string(),
                    );
                }
            }
        }

        Ok(content)
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

fn extract_content(html: &str, page_url: &str, options: &FetchOptions) -> ExtractedContent {
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
            logfire::info!(
                "htmd output oversized, retrying with readability",
                htmd_len = md.len() as u64,
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
        let article_paragraphs = "<p>This is important article content that \
            discusses 3D printer reviews in detail with specs and benchmarks. \
            </p>"
            .repeat(1500);
        let sidebar = "<div class=\"ad\">Buy now! Special offer!</div>".repeat(200);
        let comments = "<p>User comment filler text. </p>".repeat(200);
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
