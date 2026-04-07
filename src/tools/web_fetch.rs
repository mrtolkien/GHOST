use std::path::Path;

use super::output::ToolOutput;
use async_trait::async_trait;
use serde_json::{Value, json};
use tempfile::TempDir;
use url::Url;

use crate::config::DoclingConfig;
use crate::constants::MAX_EXTRACT_CHARS;
use crate::docling::{ConvertOptions, DoclingSource, convert_hybrid};
use crate::providers::ToolDefinition;
use crate::web::{ExtractedContent, FetchOptions, WebError, fetch, save_fetch_cache};

use super::context::ToolContext;
use super::error::ToolError;
use super::manager::Tool;

const PDF_CONTENT_TYPE: &str = "application/pdf";
const TEMP_PDF_FILENAME: &str = "downloaded.pdf";

#[derive(Debug)]
pub struct WebFetch;

#[async_trait]
impl Tool for WebFetch {
    fn name(&self) -> &str {
        "web_fetch"
    }

    fn schema(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Fetch and extract the text content of a web page as markdown. \
                          Uses a headless browser — works on JS-rendered SPAs, pages \
                          that block bots, and dynamic content. Content is cached for \
                          later reference curation. Works well by default; use optional \
                          parameters only when default extraction is insufficient."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "The URL to fetch."
                    },
                    "css_selector": {
                        "type": "string",
                        "description": "CSS selector to focus extraction on a specific region (e.g. 'article', '#main-content', '.post-body'). Reduces noise from sidebars and menus."
                    },
                    "scan_full_page": {
                        "type": "boolean",
                        "description": "Scroll the entire page to trigger lazy-loaded content. Adds 10-20s. Only use for infinite-scroll pages or long lists where bottom content is missing. Default: false."
                    },
                    "wait_for": {
                        "type": "string",
                        "description": "Wait condition before extracting: CSS selector (css:.loaded) or JS expression (js:() => document.querySelector('.data')). Use when content loads asynchronously after page load."
                    }
                },
                "required": ["url"]
            }),
        }
    }

    async fn execute(&self, params: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let url = params.get("url").and_then(Value::as_str).ok_or_else(|| {
            ToolError::InvalidParams("missing required parameter: url".to_string())
        })?;

        let options = FetchOptions {
            wait_for: params
                .get("wait_for")
                .and_then(Value::as_str)
                .map(String::from),
            css_selector: params
                .get("css_selector")
                .and_then(Value::as_str)
                .map(String::from),
            scan_full_page: params
                .get("scan_full_page")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            ..Default::default()
        };

        let cdp_url = ctx.browser_manager.lock().await.active_cdp_url();
        let content = match fetch(
            url,
            &options,
            ctx.config.web.crawl4ai_url.as_deref(),
            cdp_url.as_deref(),
        )
        .await
        {
            Ok(c) => c,
            Err(WebError::UnsupportedContentType { content_type }) => {
                if is_pdf_content_type(&content_type) {
                    fetch_pdf_content(url, &ctx.workspace, &ctx.config.docling).await?
                } else {
                    return Err(ToolError::ExecutionFailed(format!(
                        "This URL returned {content_type} which web_fetch cannot read. \
                         Download the file first with: \
                         `curl -L -o uploads/<filename> '{url}'`, then import with: \
                         `ghost document import file --path uploads/<filename> --topic <name>` \
                         (run the import with background: true)."
                    )));
                }
            }
            Err(e) => return Err(ToolError::ExecutionFailed(e.to_string())),
        };

        Ok(format_tool_output(url, ctx, &content))
    }
}

async fn fetch_pdf_content(
    url: &str,
    workspace: &Path,
    docling_config: &DoclingConfig,
) -> Result<ExtractedContent, ToolError> {
    let markdown = if docling_config.url.is_some() {
        convert_hybrid(
            docling_config,
            workspace,
            DoclingSource::Url { url },
            &ConvertOptions::default(),
            None,
            None,
        )
        .await
        .map_err(|error| ToolError::ExecutionFailed(format!("PDF extraction failed: {error}")))?
    } else {
        let temp_dir = tempfile::tempdir().map_err(pdf_temp_dir_error)?;
        let pdf_path = download_pdf_to_temp(url, &temp_dir).await?;
        convert_hybrid(
            docling_config,
            workspace,
            DoclingSource::File {
                path: pdf_path.as_path(),
            },
            &ConvertOptions::default(),
            None,
            None,
        )
        .await
        .map_err(|error| ToolError::ExecutionFailed(format!("PDF extraction failed: {error}")))?
    };

    Ok(markdown_to_content(markdown))
}

async fn download_pdf_to_temp(
    url: &str,
    temp_dir: &TempDir,
) -> Result<std::path::PathBuf, ToolError> {
    let response = reqwest::Client::new()
        .get(url)
        .send()
        .await
        .map_err(|error| ToolError::ExecutionFailed(format!("PDF download failed: {error}")))?;
    if !response.status().is_success() {
        return Err(ToolError::ExecutionFailed(format!(
            "PDF download failed with HTTP {} for {url}",
            response.status().as_u16()
        )));
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|error| ToolError::ExecutionFailed(format!("PDF download failed: {error}")))?;
    let path = temp_dir.path().join(pdf_filename_from_url(url));
    tokio::fs::write(&path, bytes)
        .await
        .map_err(|error| ToolError::ExecutionFailed(format!("failed to save PDF: {error}")))?;
    Ok(path)
}

fn pdf_filename_from_url(url: &str) -> String {
    let Some(parsed) = Url::parse(url).ok() else {
        return TEMP_PDF_FILENAME.to_string();
    };
    let Some(last_segment) = parsed
        .path_segments()
        .and_then(|segments| segments.filter(|segment| !segment.is_empty()).next_back())
    else {
        return TEMP_PDF_FILENAME.to_string();
    };
    if last_segment.ends_with(".pdf") {
        return last_segment.to_string();
    }
    TEMP_PDF_FILENAME.to_string()
}

fn markdown_to_content(markdown: String) -> ExtractedContent {
    let text = markdown.replace('\0', "");
    let (text, truncated) = truncate(text, MAX_EXTRACT_CHARS);
    let word_count = text.split_whitespace().count();
    ExtractedContent {
        title: None,
        text,
        word_count,
        truncated,
    }
}

fn truncate(text: String, max_chars: usize) -> (String, bool) {
    let mut chars = text.chars();
    let truncated_text: String = chars.by_ref().take(max_chars).collect();
    let was_truncated = chars.next().is_some();
    (truncated_text, was_truncated)
}

fn is_pdf_content_type(content_type: &str) -> bool {
    content_type == PDF_CONTENT_TYPE
}

fn pdf_temp_dir_error(error: std::io::Error) -> ToolError {
    ToolError::ExecutionFailed(format!("failed to create temp dir for PDF: {error}"))
}

fn format_tool_output(url: &str, ctx: &ToolContext, content: &ExtractedContent) -> ToolOutput {
    // Cache for reflection to curate later
    if let Err(e) = save_fetch_cache(&ctx.workspace, &ctx.session_id, url, content) {
        tracing::warn!(error = e.to_string(), "failed to cache fetch result");
    }

    // Format output
    let mut output = String::new();
    if let Some(title) = &content.title {
        output.push_str(&format!("# {title}\n\n"));
    }
    output.push_str(&content.text);
    if content.truncated {
        output.push_str("\n\n[Content truncated]");
    }

    ToolOutput::text(output)
}
