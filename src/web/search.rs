use std::sync::OnceLock;
use std::time::{Duration, Instant};

use reqwest::header::{HeaderMap, HeaderValue};
use serde::Deserialize;
use tokio::sync::Mutex;
use tokio::time::sleep;

use super::{SearchResult, WebError};

const BRAVE_API_URL: &str = "https://api.search.brave.com/res/v1/web/search";
const MIN_INTERVAL: Duration = Duration::from_secs(1);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const RATE_LIMIT_RETRY_DELAY: Duration = Duration::from_millis(1100);

static BRAVE_LAST_REQUEST: OnceLock<Mutex<Instant>> = OnceLock::new();

fn last_request_lock() -> &'static Mutex<Instant> {
    BRAVE_LAST_REQUEST.get_or_init(|| Mutex::new(Instant::now() - Duration::from_secs(60)))
}

#[derive(Debug)]
pub struct BraveSearchProvider {
    client: reqwest::Client,
    max_results: usize,
}

impl BraveSearchProvider {
    pub fn new(api_key: &str, max_results: usize) -> Result<Self, WebError> {
        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Subscription-Token",
            HeaderValue::from_str(api_key).map_err(|_| WebError::MissingApiKey {
                name: "BRAVE_API_KEY",
            })?,
        );

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(REQUEST_TIMEOUT)
            .build()?;

        Ok(Self {
            client,
            max_results,
        })
    }

    /// Execute a web search via the Brave Search API with rate limiting and retries.
    #[tracing::instrument(name = "search web", skip_all, fields(query = %query))]
    pub async fn search(&self, query: &str) -> Result<Vec<SearchResult>, WebError> {
        for attempt in 0..3 {
            self.wait_for_slot().await;

            let response = self
                .client
                .get(BRAVE_API_URL)
                .query(&[("q", query), ("count", &self.max_results.to_string())])
                .send()
                .await
                .map_err(WebError::Request)?;

            if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
                let retry_after = response
                    .headers()
                    .get(reqwest::header::RETRY_AFTER)
                    .and_then(|h| h.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok())
                    .map(Duration::from_secs)
                    .unwrap_or(RATE_LIMIT_RETRY_DELAY);
                if attempt < 2 {
                    tracing::warn!(
                        attempt = attempt + 1,
                        delay_ms = retry_after.as_millis() as u64,
                        "brave search rate limited, retrying",
                    );
                    sleep(retry_after).await;
                    continue;
                }
                return Err(WebError::SearchApi {
                    status: 429,
                    body: "rate limited after retries".to_string(),
                });
            }

            if !response.status().is_success() {
                let status = response.status().as_u16();
                let body = response.text().await.unwrap_or_default();
                return Err(WebError::SearchApi { status, body });
            }

            let payload: BraveSearchResponse = response.json().await.map_err(WebError::Request)?;

            let results: Vec<SearchResult> = payload
                .web
                .and_then(|w| w.results)
                .unwrap_or_default()
                .into_iter()
                .map(|item| SearchResult {
                    title: item.title.unwrap_or_else(|| "(untitled)".to_string()),
                    url: item.url.unwrap_or_default(),
                    snippet: item.description,
                    engines: None,
                    positions: None,
                    score: None,
                    published_date: None,
                })
                .collect();

            tracing::info!(
                query = query.to_string(),
                result_count = results.len() as u64,
                "brave search complete",
            );

            return Ok(results);
        }

        unreachable!("loop always returns")
    }

    async fn wait_for_slot(&self) {
        let mut last = last_request_lock().lock().await;
        let elapsed = last.elapsed();
        if elapsed < MIN_INTERVAL {
            sleep(MIN_INTERVAL - elapsed).await;
        }
        *last = Instant::now();
    }
}

#[derive(Debug, Deserialize)]
struct BraveSearchResponse {
    #[serde(default)]
    web: Option<BraveWebResults>,
}

#[derive(Debug, Deserialize)]
struct BraveWebResults {
    #[serde(default)]
    results: Option<Vec<BraveWebResult>>,
}

#[derive(Debug, Deserialize)]
struct BraveWebResult {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    description: Option<String>,
}
