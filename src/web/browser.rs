use serde_json::{Value, json};

use super::fetch::client;
use super::types::WebError;

/// Options the agent can pass to control crawl4ai behavior.
#[derive(Debug, Default)]
pub struct Crawl4aiOptions {
    pub wait_for: Option<String>,
    pub css_selector: Option<String>,
    pub scan_full_page: bool,
}

/// Build crawler_config params for crawl4ai.
///
/// Generic config: tag exclusions, word-count thresholds, and a pruning
/// content filter to reduce navigation/ad noise. No domain-specific rules —
/// the PruningContentFilter handles content extraction heuristically.
fn crawler_params(options: &Crawl4aiOptions) -> Value {
    let mut params = json!({
        "cache_mode": "bypass",
        "scan_full_page": options.scan_full_page,
        "wait_until": "networkidle",
        "page_timeout": 60000,
        "delay_before_return_html": 0.5,
        "remove_overlay_elements": true,
        "excluded_tags": ["nav", "footer", "header"],
        "word_count_threshold": 10,
        "exclude_external_links": true,
        "markdown_generator": {
            "type": "DefaultMarkdownGenerator",
            "params": {
                "content_filter": {
                    "type": "PruningContentFilter",
                    "params": {
                        "threshold": 0.4,
                        "threshold_type": "fixed",
                        "min_word_threshold": 0
                    }
                }
            }
        }
    });

    if let Some(wait_for) = &options.wait_for {
        params["wait_for"] = json!(wait_for);
    }
    if let Some(css_selector) = &options.css_selector {
        params["css_selector"] = json!(css_selector);
    }

    params
}

#[tracing::instrument(name = "fetch url crawl4ai", skip_all, fields(url = %page_url))]
pub async fn fetch_with_crawl4ai(
    base_url: &str,
    page_url: &str,
    options: &Crawl4aiOptions,
) -> Result<String, WebError> {
    let endpoint = format!("{}/crawl", base_url.trim_end_matches('/'));

    let params = crawler_params(options);
    let body = json!({
        "urls": [page_url],
        "browser_config": {
            "type": "BrowserConfig",
            "params": { "headless": true }
        },
        "crawler_config": {
            "type": "CrawlerRunConfig",
            "params": params
        }
    });

    let response = client()
        .post(&endpoint)
        .json(&body)
        .timeout(std::time::Duration::from_secs(90))
        .send()
        .await
        .map_err(|e| WebError::Crawl4ai {
            url: page_url.to_string(),
            detail: e.to_string(),
        })?;

    if !response.status().is_success() {
        let status = response.status().as_u16();
        let body = response.text().await.unwrap_or_default();
        return Err(WebError::Crawl4ai {
            url: page_url.to_string(),
            detail: format!("HTTP {status}: {body}"),
        });
    }

    let json: serde_json::Value = response.json().await.map_err(|e| WebError::Crawl4ai {
        url: page_url.to_string(),
        detail: format!("failed to parse response: {e}"),
    })?;

    // Prefer fit_markdown (filtered) over raw_markdown (noisy).
    let markdown = json
        .pointer("/results/0/markdown/fit_markdown")
        .or_else(|| json.pointer("/results/0/markdown/raw_markdown"))
        .or_else(|| json.pointer("/results/0/markdown"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| WebError::Crawl4ai {
            url: page_url.to_string(),
            detail: "no markdown in response".to_string(),
        })?;

    logfire::info!(
        "crawl4ai fetch complete",
        url = page_url.to_string(),
        markdown_len = markdown.len() as u64,
    );

    Ok(markdown.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crawler_params_defaults() {
        let params = crawler_params(&Crawl4aiOptions::default());
        assert_eq!(params["wait_until"], "networkidle");
        assert_eq!(params["scan_full_page"], false);
        assert_eq!(params["remove_overlay_elements"], true);
        assert_eq!(params["delay_before_return_html"], 0.5);
        assert!(params.get("wait_for").is_none());
        assert!(params.get("css_selector").is_none());
        assert!(params["markdown_generator"]["params"]["content_filter"].is_object());
    }

    #[test]
    fn crawler_params_with_options() {
        let opts = Crawl4aiOptions {
            wait_for: Some("css:.loaded".into()),
            css_selector: Some("article.main".into()),
            scan_full_page: true,
        };
        let params = crawler_params(&opts);
        assert_eq!(params["wait_for"], "css:.loaded");
        assert_eq!(params["css_selector"], "article.main");
        assert_eq!(params["scan_full_page"], true);
    }
}
