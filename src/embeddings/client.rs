use serde::Deserialize;

use crate::config::EmbeddingsConfig;

use super::error::EmbeddingError;

const EMBED_REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);
const EMBED_HEALTH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

#[derive(Debug, Clone)]
pub struct EmbeddingClient {
    url: String,
    model: String,
    dimension: usize,
    batch_size: usize,
    client: reqwest::Client,
}

impl EmbeddingClient {
    pub fn new(config: &EmbeddingsConfig) -> Self {
        Self {
            url: config.url.trim_end_matches('/').to_string(),
            model: config.model.clone(),
            dimension: config.dimension,
            batch_size: config.batch_size,
            client: reqwest::Client::builder()
                .timeout(EMBED_REQUEST_TIMEOUT)
                .build()
                .expect("failed to build reqwest client"),
        }
    }

    pub fn dimension(&self) -> usize {
        self.dimension
    }

    pub fn batch_size(&self) -> usize {
        self.batch_size
    }

    #[tracing::instrument(name = "embed batch", skip_all, fields(
        model = %self.model,
        batch_size = inputs.len(),
    ))]
    pub async fn embed_batch(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        if inputs.is_empty() {
            return Ok(Vec::new());
        }

        let url = format!("{}/v1/embeddings", self.url);
        let body = EmbedRequest {
            model: self.model.clone(),
            input: inputs.to_vec(),
            dimensions: Some(self.dimension),
        };

        let response = self.client.post(&url).json(&body).send().await?;
        let status = response.status();

        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(EmbeddingError::Api {
                status: status.as_u16(),
                body: text,
            });
        }

        let payload: EmbedResponse = response.json().await?;

        if payload.data.is_empty() {
            return Err(EmbeddingError::EmptyResponse);
        }

        let vectors: Vec<Vec<f32>> = payload.data.into_iter().map(|d| d.embedding).collect();

        for v in &vectors {
            if v.len() != self.dimension {
                return Err(EmbeddingError::DimensionMismatch {
                    expected: self.dimension,
                    actual: v.len(),
                });
            }
        }

        Ok(vectors)
    }

    pub async fn is_available(&self) -> bool {
        let url = format!("{}/v1/models", self.url);
        self.client
            .get(&url)
            .timeout(EMBED_HEALTH_TIMEOUT)
            .send()
            .await
            .is_ok_and(|r| r.status().is_success())
    }
}

// -- OpenAI-compatible wire types --

#[derive(Debug, serde::Serialize)]
struct EmbedRequest {
    model: String,
    input: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dimensions: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct EmbedResponse {
    data: Vec<EmbedObject>,
}

#[derive(Debug, Deserialize)]
struct EmbedObject {
    embedding: Vec<f32>,
}
