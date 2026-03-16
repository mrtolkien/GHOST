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
/// Uses `domcontentloaded` (not `networkidle` — most sites with ads/trackers
/// never reach network idle and time out). No PruningContentFilter (unreliable,
/// returns empty on Wikipedia). No `remove_overlay_elements` (strips actual
/// content on Wikipedia). We rely on excluded_tags + word_count_threshold for
/// noise reduction, and our own 120K char truncation for size capping.
fn crawler_params(options: &Crawl4aiOptions) -> Value {
    let mut params = json!({
        "cache_mode": "bypass",
        "scan_full_page": options.scan_full_page,
        "wait_until": "domcontentloaded",
        "page_timeout": 60000,
        "delay_before_return_html": 0.5,
        "excluded_tags": ["nav", "footer", "header"],
        "word_count_threshold": 10,
        "exclude_external_links": true
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
    cdp_url: Option<&str>,
) -> Result<String, WebError> {
    let endpoint = format!("{}/crawl", base_url.trim_end_matches('/'));

    let params = crawler_params(options);
    let mut browser_params = json!({ "headless": true });
    if let Some(base_url) = cdp_url {
        // Resolve to the full webSocketDebuggerUrl — Crawl4AI uses
        // Playwright's connect_over_cdp which needs the full path,
        // unlike chromiumoxide which auto-discovers from base URLs.
        let ws_url = crate::web::browser::cdp::resolve_ws_url(base_url)
            .await
            .map_err(|e| WebError::Crawl4ai {
                url: page_url.to_string(),
                detail: format!("failed to resolve CDP URL: {e}"),
            })?;
        // The resolved URL may have 127.0.0.1 (Chrome's self-view).
        // Replace the host with the original base_url's host so the
        // URL is reachable from Crawl4AI's network perspective.
        let ws_url = rewrite_ws_host(&ws_url, base_url);
        browser_params["cdp_url"] = json!(ws_url);
        // Share the existing browser's session (cookies, localStorage)
        // instead of creating an isolated context.
        browser_params["use_persistent_context"] = json!(true);
    }
    let body = json!({
        "urls": [page_url],
        "browser_config": {
            "type": "BrowserConfig",
            "params": browser_params
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

    let markdown = json
        .pointer("/results/0/markdown/raw_markdown")
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

/// Replace the host:port in `resolved_url` with the host:port from
/// `original_url`.
///
/// Chrome's `/json/version` returns `webSocketDebuggerUrl` with its own
/// view of its address (usually `127.0.0.1`). When Crawl4AI runs in a
/// different network context (e.g. Docker), it needs the host from the
/// original configured URL to actually reach Chrome.
fn rewrite_ws_host(resolved_url: &str, original_url: &str) -> String {
    let Ok(mut resolved) = url::Url::parse(resolved_url) else {
        return resolved_url.to_string();
    };
    let Ok(original) = url::Url::parse(original_url) else {
        return resolved_url.to_string();
    };
    if let Some(host) = original.host_str() {
        let _ = resolved.set_host(Some(host));
    }
    if let Some(port) = original.port() {
        let _ = resolved.set_port(Some(port));
    }
    resolved.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crawler_params_defaults() {
        let params = crawler_params(&Crawl4aiOptions::default());
        assert_eq!(params["wait_until"], "domcontentloaded");
        assert_eq!(params["scan_full_page"], false);
        assert!(params.get("remove_overlay_elements").is_none());
        assert_eq!(params["delay_before_return_html"], 0.5);
        assert!(params.get("wait_for").is_none());
        assert!(params.get("css_selector").is_none());
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

    #[test]
    fn rewrite_ws_host_replaces_host_and_port() {
        let resolved = "ws://127.0.0.1:9222/devtools/browser/abc-123";
        let original = "ws://172.19.0.4:9222";
        let result = rewrite_ws_host(resolved, original);
        assert_eq!(result, "ws://172.19.0.4:9222/devtools/browser/abc-123");
    }

    #[test]
    fn rewrite_ws_host_different_port() {
        let resolved = "ws://127.0.0.1:9222/devtools/browser/x";
        let original = "ws://chrome-host:9333";
        let result = rewrite_ws_host(resolved, original);
        assert_eq!(result, "ws://chrome-host:9333/devtools/browser/x");
    }

    #[test]
    fn rewrite_ws_host_same_host_noop() {
        let resolved = "ws://localhost:9222/devtools/browser/x";
        let original = "ws://localhost:9222";
        let result = rewrite_ws_host(resolved, original);
        assert_eq!(result, "ws://localhost:9222/devtools/browser/x");
    }
}
