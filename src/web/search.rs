use std::sync::OnceLock;
use std::time::{Duration, Instant};

use reqwest::header::{HeaderMap, HeaderValue};
use serde::Deserialize;
use tokio::sync::Mutex;
use tokio::time::sleep;

use super::{SearchResult, WebError};

const BRAVE_API_URL: &str = "https://api.search.brave.com/res/v1/web/search";
const MIN_INTERVAL: Duration = Duration::from_secs(1);

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
            .timeout(Duration::from_secs(15))
            .build()?;

        Ok(Self {
            client,
            max_results,
        })
    }

    #[tracing::instrument(skip_all, fields(query = %query))]
    pub async fn search(&self, query: &str) -> Result<Vec<SearchResult>, WebError> {
        match self.execute_request(query).await {
            Ok(results) => Ok(results),
            Err(RateLimited(delay)) => {
                logfire::warn!(
                    "brave search rate limited, retrying",
                    delay_ms = delay.as_millis() as u64,
                );
                sleep(delay).await;
                self.execute_request(query)
                    .await
                    .map_err(|e| e.into_web_error())
            }
            Err(ApiError(e)) => Err(e),
        }
    }

    async fn execute_request(&self, query: &str) -> Result<Vec<SearchResult>, SearchRequestError> {
        self.wait_for_slot().await;

        let response = self
            .client
            .get(BRAVE_API_URL)
            .query(&[("q", query), ("count", &self.max_results.to_string())])
            .send()
            .await
            .map_err(|e| ApiError(WebError::Request(e)))?;

        if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let retry_after = response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|h| h.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok())
                .map(Duration::from_secs)
                .unwrap_or(Duration::from_millis(1100));
            return Err(RateLimited(retry_after));
        }

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let body = response.text().await.unwrap_or_default();
            return Err(ApiError(WebError::SearchApi { status, body }));
        }

        let payload: BraveSearchResponse = response
            .json()
            .await
            .map_err(|e| ApiError(WebError::Request(e)))?;

        let results: Vec<SearchResult> = payload
            .web
            .and_then(|w| w.results)
            .unwrap_or_default()
            .into_iter()
            .map(|item| SearchResult {
                title: item.title.unwrap_or_else(|| "(untitled)".to_string()),
                url: item.url.unwrap_or_default(),
                snippet: item.description,
            })
            .collect();

        logfire::info!(
            "brave search complete",
            query = query.to_string(),
            result_count = results.len() as u64,
        );

        Ok(results)
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

/// Internal error for distinguishing rate limits from real errors during retry.
enum SearchRequestError {
    RateLimited(Duration),
    ApiError(WebError),
}

use SearchRequestError::*;

impl SearchRequestError {
    fn into_web_error(self) -> WebError {
        match self {
            RateLimited(_) => WebError::SearchApi {
                status: 429,
                body: "rate limited after retry".to_string(),
            },
            ApiError(e) => e,
        }
    }
}

impl From<SearchRequestError> for WebError {
    fn from(e: SearchRequestError) -> Self {
        e.into_web_error()
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
