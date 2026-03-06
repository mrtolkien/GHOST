use async_trait::async_trait;
use serde_json::{Value, json};

use crate::providers::ToolDefinition;
use crate::web::{FetchOptions, WebError, fetch, save_fetch_cache};

use super::context::ToolContext;
use super::error::ToolError;
use super::manager::Tool;

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
            description: "Fetch and extract the text content of a web page. Uses a \
                          headless browser for JavaScript-rendered pages. Content is \
                          automatically cached for later reference curation.\n\n\
                          Options:\n\
                          - wait_for: wait for a CSS selector (css:.content) or JS \
                            condition (js:() => ...) before extracting. Use when \
                            content loads dynamically.\n\
                          - css_selector: restrict extraction to a specific DOM region \
                            (e.g. 'article', 'main', '#content'). Reduces noise.\n\
                          - scan_full_page: scroll the entire page to trigger \
                            lazy-loaded content. Slower — only use for infinite-scroll \
                            or long list pages."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "The URL to fetch."
                    },
                    "wait_for": {
                        "type": "string",
                        "description": "CSS selector (css:<sel>) or JS condition (js:<code>) to wait for before extraction."
                    },
                    "css_selector": {
                        "type": "string",
                        "description": "CSS selector to focus extraction on (e.g. 'article', '#main-content')."
                    },
                    "scan_full_page": {
                        "type": "boolean",
                        "description": "Scroll full page for lazy-loaded content. Default: false."
                    }
                },
                "required": ["url"]
            }),
        }
    }

    async fn execute(&self, params: Value, ctx: &ToolContext) -> Result<String, ToolError> {
        let url = params.get("url").and_then(Value::as_str).ok_or_else(|| {
            ToolError::InvalidParams("missing required parameter: url".to_string())
        })?;

        let options = FetchOptions {
            wait_for: params.get("wait_for").and_then(Value::as_str).map(String::from),
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

        let content = match fetch(url, &options, ctx.config.web.crawl4ai_url.as_deref()).await {
            Ok(c) => c,
            Err(WebError::UnsupportedContentType { content_type }) => {
                return Err(ToolError::ExecutionFailed(format!(
                    "This URL returned {content_type} which web_fetch cannot read. \
                     Import it directly with: \
                     `ghost reference import --source page --url '{url}' --topic <name>` \
                     (run with background: true). \
                     Do NOT curl the file — page import handles PDFs and binary \
                     documents via docling automatically."
                )));
            }
            Err(e) => return Err(ToolError::ExecutionFailed(e.to_string())),
        };

        // Cache for reflection to curate later
        if let Err(e) = save_fetch_cache(&ctx.workspace, &ctx.session_id, url, &content) {
            logfire::warn!("failed to cache fetch result", error = e.to_string(),);
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

        Ok(output)
    }
}
