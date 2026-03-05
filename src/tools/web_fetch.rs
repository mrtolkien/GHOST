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
            description: "Fetch and extract the text content of a web page. Use this \
                          after web_search to read promising results in full. Content \
                          is automatically cached for later reference curation."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "The URL to fetch."
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

        let options = FetchOptions::default();

        let content = match fetch(url, &options, ctx.config.web.crawl4ai_url.as_deref()).await
        {
            Ok(c) => c,
            Err(WebError::UnsupportedContentType { content_type }) => {
                return Err(ToolError::ExecutionFailed(format!(
                    "This URL returned {content_type} which web_fetch cannot read. \
                     Read the reference-import skill — it can import PDFs, DOCX, \
                     and other binary documents into your knowledge base."
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
