use async_trait::async_trait;
use serde_json::{Value, json};

use crate::providers::ToolDefinition;
use crate::web::{FetchOptions, fetch, save_fetch_cache};

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
                    },
                    "max_chars": {
                        "type": "integer",
                        "description": "Maximum characters to extract (default: 50000)."
                    },
                    "readability": {
                        "type": "boolean",
                        "description": "Use Readability to extract article content \
                                        only, stripping navigation and sidebars. \
                                        ALWAYS USE IT FOR SINGLE ARTICLES (default: false)."
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

        let max_chars = params
            .get("max_chars")
            .and_then(Value::as_u64)
            .map(|n| n as usize)
            .unwrap_or(50_000);

        let readability = params
            .get("readability")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        let options = FetchOptions {
            max_chars,
            readability,
            raw: false,
        };

        let content = fetch(url, &options, ctx.config.web.crawl4ai_url.as_deref())
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        // Cache for reflection to curate later
        if let Err(e) = save_fetch_cache(&ctx.workspace, url, &content) {
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
