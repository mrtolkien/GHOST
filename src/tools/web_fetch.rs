use super::output::ToolOutput;
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
            description: "Fetch and extract the text content of a web page as markdown. \
                          Uses a headless browser — works on JS-rendered SPAs, pages \
                          that block bots, and dynamic content. Content is cached for \
                          later reference curation.\n\n\
                          Works well by default for most pages (articles, docs, forums, \
                          product reviews, Stack Overflow, Wikipedia). Only use the \
                          options below when the default extraction is insufficient:\n\n\
                          - css_selector: focus extraction on a DOM region (e.g. \
                            'article', '#main-content', '.post-body'). Use when output \
                            has too much sidebar/menu noise.\n\
                          - scan_full_page: scroll the entire page before extracting. \
                            Use ONLY for infinite-scroll pages or long lists where \
                            content near the bottom is missing. Adds 10-20s.\n\
                          - wait_for: wait for a CSS selector (css:.loaded) or JS \
                            condition (js:() => document.querySelector('.data')) \
                            before extracting. Use when content loads after an async \
                            fetch or animation."
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
                return Err(ToolError::ExecutionFailed(format!(
                    "This URL returned {content_type} which web_fetch cannot read. \
                     Import it directly with: \
                     `ghost document import url --url '{url}' --topic <name>` \
                     (run with background: true). \
                     Do NOT curl the file — document import handles PDFs and binary \
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

        Ok(ToolOutput::text(output))
    }
}
