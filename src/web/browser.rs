use serde_json::json;

use super::fetch::client;
use super::types::WebError;

#[tracing::instrument(skip_all, fields(url = %page_url))]
pub async fn fetch_with_crawl4ai(base_url: &str, page_url: &str) -> Result<String, WebError> {
    let endpoint = format!("{}/crawl", base_url.trim_end_matches('/'));

    let body = json!({
        "urls": [page_url],
        "browser_config": {
            "type": "BrowserConfig",
            "params": { "headless": true }
        },
        "crawler_config": {
            "type": "CrawlerRunConfig",
            "params": {
                "cache_mode": "bypass",
                "scan_full_page": true,
                "wait_until": "domcontentloaded",
                "page_timeout": 60000,
                "delay_before_return_html": 2.0
            }
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
        .ok_or_else(|| WebError::Crawl4ai {
            url: page_url.to_string(),
            detail: "no markdown in response".to_string(),
        })?;

    if markdown.is_empty() {
        return Err(WebError::Crawl4ai {
            url: page_url.to_string(),
            detail: "crawl4ai returned empty markdown".to_string(),
        });
    }

    logfire::info!(
        "crawl4ai fetch complete",
        url = page_url.to_string(),
        markdown_len = markdown.len() as u64,
    );

    Ok(markdown.to_string())
}
