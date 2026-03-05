use std::path::Path;

use reqwest::multipart;

use super::WebError;

/// Convert a local file to markdown via docling-serve.
#[tracing::instrument(name = "docling convert file", skip_all, fields(path = %path.display()))]
pub async fn convert_file(docling_url: &str, path: &Path) -> Result<String, WebError> {
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file")
        .to_string();
    let file_bytes = tokio::fs::read(path)
        .await
        .map_err(|e| WebError::Docling(format!("failed to read file: {e}")))?;

    let part = multipart::Part::bytes(file_bytes)
        .file_name(file_name)
        .mime_str("application/octet-stream")
        .map_err(|e| WebError::Docling(e.to_string()))?;

    let form = multipart::Form::new().part("files", part).text(
        "options",
        serde_json::json!({"to_formats": ["md"]}).to_string(),
    );

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{docling_url}/v1/convert/file"))
        .multipart(form)
        .send()
        .await
        .map_err(|e| WebError::Docling(format!("request failed: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(WebError::Docling(format!("HTTP {status}: {body}")));
    }

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| WebError::Docling(format!("invalid JSON response: {e}")))?;

    extract_markdown_from_response(&body)
}

/// Convert a URL-hosted document to markdown via docling-serve.
#[tracing::instrument(name = "docling convert url", skip_all, fields(%url))]
pub async fn convert_url(docling_url: &str, url: &str) -> Result<String, WebError> {
    let payload = serde_json::json!({
        "sources": [{"kind": "http", "url": url}],
        "options": {"to_formats": ["md"]}
    });

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{docling_url}/v1/convert/source"))
        .json(&payload)
        .send()
        .await
        .map_err(|e| WebError::Docling(format!("request failed: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(WebError::Docling(format!("HTTP {status}: {body}")));
    }

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| WebError::Docling(format!("invalid JSON response: {e}")))?;

    extract_markdown_from_response(&body)
}

fn extract_markdown_from_response(body: &serde_json::Value) -> Result<String, WebError> {
    if let Some(md) = body.pointer("/document/md_content").and_then(|v| v.as_str()) {
        return Ok(md.to_string());
    }
    Err(WebError::Docling(format!(
        "could not extract markdown from response: {}",
        serde_json::to_string_pretty(body).unwrap_or_default()
    )))
}
