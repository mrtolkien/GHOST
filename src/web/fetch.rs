use std::sync::OnceLock;

use reqwest::header::CONTENT_TYPE;
use url::Url;

use super::{ExtractedContent, FetchOptions, WebError};

static HTTP_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

fn client() -> &'static reqwest::Client {
    HTTP_CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("failed to build HTTP client")
    })
}

#[tracing::instrument(skip_all, fields(url = %url))]
pub async fn fetch(url: &str, options: &FetchOptions) -> Result<ExtractedContent, WebError> {
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
        Ok(extract_content(&raw_text, url, options))
    } else {
        let text = raw_text.replace('\0', "");
        let (text, truncated) = truncate(text, options.max_chars);
        let word_count = text.split_whitespace().count();
        Ok(ExtractedContent {
            title: None,
            text,
            word_count,
            truncated,
        })
    }
}

fn extract_content(html: &str, page_url: &str, options: &FetchOptions) -> ExtractedContent {
    if options.raw {
        let text = html.replace('\0', "");
        let (text, truncated) = truncate(text, options.max_chars);
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
        (None, html_to_markdown(html))
    };

    let text = text.replace('\0', "");
    let (text, truncated) = truncate(text, options.max_chars);
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
    htmd::convert(html).unwrap_or_else(|_| html.to_string())
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
        assert!(result.text.contains("About"));
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
    fn truncation_at_max_chars() {
        let html = "<p>Hello world this is a test</p>";
        let options = FetchOptions {
            max_chars: 10,
            ..Default::default()
        };
        let result = extract_content(html, "http://example.com", &options);
        assert!(result.text.chars().count() <= 10);
        assert!(result.truncated);
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
}
