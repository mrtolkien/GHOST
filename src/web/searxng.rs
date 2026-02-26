use std::time::Duration;

use serde::Deserialize;

use super::{SearchResult, WebError};

#[derive(Debug)]
pub struct SearxngSearchProvider {
    client: reqwest::Client,
    base_url: String,
    max_results: usize,
}

impl SearxngSearchProvider {
    pub fn new(base_url: &str, max_results: usize) -> Result<Self, WebError> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()?;

        Ok(Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            max_results,
        })
    }

    #[tracing::instrument(name = "search web", skip_all, fields(
        query = %query,
        provider = "searxng",
    ))]
    pub async fn search(&self, query: &str) -> Result<Vec<SearchResult>, WebError> {
        let response = self
            .client
            .get(format!("{}/search", self.base_url))
            .query(&[("q", query), ("format", "json")])
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let body = response.text().await.unwrap_or_default();
            return Err(WebError::SearchApi { status, body });
        }

        let payload: SearxngResponse = response.json().await?;

        let results: Vec<SearchResult> = payload
            .results
            .into_iter()
            .take(self.max_results)
            .map(|item| SearchResult {
                title: item.title,
                url: item.url,
                snippet: item.content.filter(|s| !s.is_empty()),
                engines: Some(item.engines),
                positions: Some(item.positions),
                score: Some(item.score),
                published_date: item.published_date.filter(|s| !s.is_empty()),
            })
            .collect();

        logfire::info!(
            "searxng search complete",
            query = query.to_string(),
            result_count = results.len() as u64,
        );

        Ok(results)
    }
}

#[derive(Debug, Deserialize)]
struct SearxngResponse {
    #[serde(default)]
    results: Vec<SearxngResult>,
}

#[derive(Debug, Deserialize)]
struct SearxngResult {
    #[serde(default)]
    title: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    engines: Vec<String>,
    #[serde(default)]
    positions: Vec<u32>,
    #[serde(default)]
    score: f64,
    #[serde(default, rename = "publishedDate")]
    published_date: Option<String>,
}
