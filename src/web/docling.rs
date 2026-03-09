use std::path::Path;
use std::time::Duration;

use base64::Engine;
use serde_json::json;

use crate::config::DoclingConfig;

use super::WebError;

/// What to convert.
pub enum DoclingSource<'a> {
    File { path: &'a Path },
    Url { url: &'a str },
}

/// Caller-facing options. Hardcoded defaults (ocr_engine, table_mode,
/// image_export_mode) are set internally.
pub struct ConvertOptions {
    pub ocr: bool,
    pub page_range: Option<(u32, u32)>,
}

impl Default for ConvertOptions {
    fn default() -> Self {
        Self {
            ocr: true,
            page_range: None,
        }
    }
}

/// Single entry point for docling conversion (file or URL -> markdown).
#[tracing::instrument(name = "docling convert", skip_all)]
pub async fn convert(
    config: &DoclingConfig,
    source: DoclingSource<'_>,
    options: &ConvertOptions,
) -> Result<String, WebError> {
    let base_url = config.url.as_deref().ok_or_else(|| {
        WebError::Docling("docling URL not configured ([docling].url)".into())
    })?;
    let timeout = Duration::from_secs(config.timeout);

    // Build source JSON
    let source_json = match &source {
        DoclingSource::File { path } => {
            let file_bytes = tokio::fs::read(path)
                .await
                .map_err(|e| WebError::Docling(format!("failed to read file: {e}")))?;
            let b64 = base64::engine::general_purpose::STANDARD.encode(&file_bytes);
            let filename = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("file");
            json!({"kind": "file", "base64_string": b64, "filename": filename})
        }
        DoclingSource::Url { url } => {
            json!({"kind": "http", "url": url})
        }
    };

    // Build options JSON
    let mut opts = json!({
        "to_formats": ["md"],
        "image_export_mode": "placeholder",
        "pipeline": "standard",
        "do_ocr": options.ocr,
        "ocr_engine": "rapidocr",
        "table_mode": "accurate",
    });
    if let Some((start, end)) = options.page_range {
        opts["page_range"] = json!([start, end]);
    }

    let payload = json!({
        "sources": [source_json],
        "options": opts,
    });

    let client = reqwest::Client::new();

    // 1. Submit
    let resp = client
        .post(format!("{base_url}/v1/convert/source/async"))
        .json(&payload)
        .send()
        .await
        .map_err(|e| WebError::Docling(format!("submit failed: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(WebError::Docling(format!("submit HTTP {status}: {body}")));
    }

    let submit_body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| WebError::Docling(format!("invalid submit response: {e}")))?;
    let task_id = submit_body["task_id"]
        .as_str()
        .ok_or_else(|| WebError::Docling("missing task_id in submit response".into()))?
        .to_string();

    // 2. Poll until terminal
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if tokio::time::Instant::now() >= deadline {
            return Err(WebError::DoclingTimeout {
                seconds: config.timeout,
            });
        }

        let poll_resp = client
            .get(format!("{base_url}/v1/status/poll/{task_id}?wait=5"))
            .send()
            .await
            .map_err(|e| WebError::Docling(format!("poll failed: {e}")))?;

        let poll_body: serde_json::Value = poll_resp
            .json()
            .await
            .map_err(|e| WebError::Docling(format!("invalid poll response: {e}")))?;

        let status = poll_body["task_status"]
            .as_str()
            .unwrap_or("unknown");

        match status {
            "success" => break,
            "failure" | "error" => {
                let detail = poll_body["error_message"]
                    .as_str()
                    .unwrap_or("unknown error")
                    .to_string();
                return Err(WebError::DoclingTaskFailed { detail });
            }
            _ => continue, // "pending", "started", etc.
        }
    }

    // 3. Fetch result
    let result_resp = client
        .get(format!("{base_url}/v1/result/{task_id}"))
        .send()
        .await
        .map_err(|e| WebError::Docling(format!("result fetch failed: {e}")))?;

    if !result_resp.status().is_success() {
        let status = result_resp.status();
        let body = result_resp.text().await.unwrap_or_default();
        return Err(WebError::Docling(format!("result HTTP {status}: {body}")));
    }

    let body: serde_json::Value = result_resp
        .json()
        .await
        .map_err(|e| WebError::Docling(format!("invalid result JSON: {e}")))?;

    extract_markdown_from_response(&body)
}

fn extract_markdown_from_response(body: &serde_json::Value) -> Result<String, WebError> {
    // Try /document/md_content (single-doc response)
    if let Some(md) = body
        .pointer("/document/md_content")
        .and_then(|v| v.as_str())
    {
        return Ok(md.to_string());
    }
    // Try /output/documents/0/md_content (async multi-doc response)
    if let Some(md) = body
        .pointer("/output/documents/0/md_content")
        .and_then(|v| v.as_str())
    {
        return Ok(md.to_string());
    }
    Err(WebError::Docling(format!(
        "could not extract markdown from response: {}",
        serde_json::to_string_pretty(body).unwrap_or_default()
    )))
}
