use std::path::Path;
use std::time::Duration;

use base64::Engine;
use serde_json::json;

use crate::config::DoclingConfig;

use super::{DoclingDocument, DoclingError};

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

/// Single entry point for docling conversion (file or URL -> DoclingDocument).
///
/// Backend selection:
/// - `config.url` is `Some(url)` -> HTTP to remote docling-serve
/// - `config.url` is `None` -> local uv script (file sources only)
///
/// URL sources (e.g. from web imports) require a remote docling-serve.
/// The local script backend only supports file paths.
#[tracing::instrument(name = "docling convert", skip_all)]
pub async fn convert(
    config: &DoclingConfig,
    workspace: &Path,
    source: DoclingSource<'_>,
    options: &ConvertOptions,
) -> Result<DoclingDocument, DoclingError> {
    let timeout = Duration::from_secs(config.timeout);

    match &config.url {
        Some(url) => convert_http(url, timeout, source, options).await,
        None => convert_script(workspace, source, options, timeout).await,
    }
}

/// HTTP backend: submit -> poll -> fetch via docling-serve REST API.
async fn convert_http(
    base_url: &str,
    timeout: Duration,
    source: DoclingSource<'_>,
    options: &ConvertOptions,
) -> Result<DoclingDocument, DoclingError> {
    // Build source JSON
    let source_json = match &source {
        DoclingSource::File { path } => {
            let file_bytes = tokio::fs::read(path)
                .await
                .map_err(|e| DoclingError::Conversion(format!("failed to read file: {e}")))?;
            let b64 = base64::engine::general_purpose::STANDARD.encode(&file_bytes);
            let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
            json!({"kind": "file", "base64_string": b64, "filename": filename})
        }
        DoclingSource::Url { url } => {
            json!({"kind": "http", "url": url})
        }
    };

    // Build options JSON
    let mut opts = json!({
        "to_formats": ["json"],
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
        .map_err(|e| DoclingError::Conversion(format!("submit failed: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(DoclingError::Conversion(format!(
            "submit HTTP {status}: {body}"
        )));
    }

    let submit_body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| DoclingError::Conversion(format!("invalid submit response: {e}")))?;
    let task_id = submit_body["task_id"]
        .as_str()
        .ok_or_else(|| DoclingError::Conversion("missing task_id in submit response".into()))?
        .to_string();

    // 2. Poll until terminal
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if tokio::time::Instant::now() >= deadline {
            return Err(DoclingError::Timeout {
                seconds: timeout.as_secs(),
            });
        }

        let poll_resp = client
            .get(format!("{base_url}/v1/status/poll/{task_id}?wait=5"))
            .send()
            .await
            .map_err(|e| DoclingError::Conversion(format!("poll failed: {e}")))?;

        let poll_body: serde_json::Value = poll_resp
            .json()
            .await
            .map_err(|e| DoclingError::Conversion(format!("invalid poll response: {e}")))?;

        let status = poll_body["task_status"].as_str().unwrap_or("unknown");

        match status {
            "success" => break,
            "failure" | "error" => {
                let detail = poll_body["error_message"]
                    .as_str()
                    .unwrap_or("unknown error")
                    .to_string();
                return Err(DoclingError::TaskFailed { detail });
            }
            _ => continue, // "pending", "started", etc.
        }
    }

    // 3. Fetch result
    let result_resp = client
        .get(format!("{base_url}/v1/result/{task_id}"))
        .send()
        .await
        .map_err(|e| DoclingError::Conversion(format!("result fetch failed: {e}")))?;

    if !result_resp.status().is_success() {
        let status = result_resp.status();
        let body = result_resp.text().await.unwrap_or_default();
        return Err(DoclingError::Conversion(format!(
            "result HTTP {status}: {body}"
        )));
    }

    let body: serde_json::Value = result_resp
        .json()
        .await
        .map_err(|e| DoclingError::Conversion(format!("invalid result JSON: {e}")))?;

    extract_document_from_response(&body)
}

/// Script backend: shell out to `uv run convert.py`.
///
/// Only supports `DoclingSource::File` — URL sources require a remote
/// docling-serve (`[docling].url` must be configured).
async fn convert_script(
    workspace: &Path,
    source: DoclingSource<'_>,
    options: &ConvertOptions,
    timeout: Duration,
) -> Result<DoclingDocument, DoclingError> {
    let DoclingSource::File { path } = &source else {
        return Err(DoclingError::Conversion(
            "URL sources require [docling].url to be configured — \
             the local script backend only supports file sources"
                .into(),
        ));
    };

    // Check uv is available
    let uv_ok = tokio::process::Command::new("uv")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .map(|s| s.success())
        .unwrap_or(false);
    if !uv_ok {
        return Err(DoclingError::Conversion(
            "uv is not installed — install it from https://docs.astral.sh/uv/ \
             or configure [docling].url to use a remote docling-serve"
                .into(),
        ));
    }

    let script = workspace.join("services/docling/convert.py");
    if !script.exists() {
        return Err(DoclingError::Conversion(format!(
            "convert.py not found at {} — run `ghost setup` to install workspace services",
            script.display()
        )));
    }

    // Use a temp directory with a known filename to avoid NamedTempFile
    // platform issues (exclusive file locks on some OSes).
    let tmp_dir = tempfile::tempdir()
        .map_err(|e| DoclingError::Conversion(format!("failed to create temp dir: {e}")))?;
    let output_path = tmp_dir.path().join("output.json");

    let mut cmd = tokio::process::Command::new("uv");
    cmd.arg("run")
        .arg(&script)
        .arg("--path")
        .arg(path.as_os_str())
        .arg("--output")
        .arg(&output_path);

    if !options.ocr {
        cmd.arg("--no-ocr");
    }
    if let Some((start, end)) = options.page_range {
        cmd.arg("--page-range").arg(format!("{start}-{end}"));
    }

    tracing::info!("spawning docling conversion via uv script");

    let child = cmd
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| DoclingError::Conversion(format!("failed to spawn uv: {e}")))?;

    let result = tokio::time::timeout(timeout, child.wait_with_output())
        .await
        .map_err(|_| DoclingError::Timeout {
            seconds: timeout.as_secs(),
        })?
        .map_err(|e| DoclingError::Conversion(format!("uv process failed: {e}")))?;

    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        return Err(DoclingError::TaskFailed {
            detail: stderr.to_string(),
        });
    }

    // Log script stdout (progress messages) at debug level
    let stdout = String::from_utf8_lossy(&result.stdout);
    if !stdout.is_empty() {
        tracing::debug!(output = %stdout, "docling script output");
    }

    let json_str = tokio::fs::read_to_string(&output_path)
        .await
        .map_err(|e| DoclingError::Conversion(format!("failed to read output: {e}")))?;

    serde_json::from_str(&json_str)
        .map_err(|e| DoclingError::Parse(format!("failed to parse DoclingDocument: {e}")))
    // tmp_dir is dropped here, cleaning up the temp directory
}

fn extract_document_from_response(
    body: &serde_json::Value,
) -> Result<DoclingDocument, DoclingError> {
    // Try /document/json_content
    if let Some(json_doc) = body.pointer("/document/json_content") {
        return serde_json::from_value(json_doc.clone())
            .map_err(|e| DoclingError::Parse(e.to_string()));
    }
    // Try /output/documents/0/json_content (async multi-doc response)
    if let Some(json_doc) = body.pointer("/output/documents/0/json_content") {
        return serde_json::from_value(json_doc.clone())
            .map_err(|e| DoclingError::Parse(e.to_string()));
    }
    Err(DoclingError::Conversion(
        "could not extract json_content from response".into(),
    ))
}
