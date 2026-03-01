use async_trait::async_trait;
use serde_json::{Value, json};

use crate::config::SearchProviderConfig;
use crate::providers::ToolDefinition;
use crate::web::{
    BraveSearchProvider, SearxngSearchProvider, format_search_metadata, save_search_cache,
};

use super::context::ToolContext;
use super::error::ToolError;
use super::manager::Tool;

#[derive(Debug)]
pub struct WebSearch;

#[async_trait]
impl Tool for WebSearch {
    fn name(&self) -> &str {
        "web_search"
    }

    fn schema(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Search the web for current information. Use this to find \
                          up-to-date facts, product recommendations, news, prices, \
                          reviews, and anything that changes over time. Results are \
                          automatically cached for later reference curation."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The search query. Be specific and include \
                                        relevant context (e.g. year, product category)."
                    },
                    "max_results": {
                        "type": "integer",
                        "description": "Maximum number of results to return (default: \
                                        from config, typically 5)."
                    }
                },
                "required": ["query"]
            }),
        }
    }

    async fn execute(&self, params: Value, ctx: &ToolContext) -> Result<String, ToolError> {
        let query = params.get("query").and_then(Value::as_str).ok_or_else(|| {
            ToolError::InvalidParams("missing required parameter: query".to_string())
        })?;

        let max_results = params
            .get("max_results")
            .and_then(Value::as_u64)
            .map(|n| n as usize)
            .unwrap_or(ctx.config.web.search_max_results);

        let results = match &ctx.config.web.search_provider {
            SearchProviderConfig::Brave => {
                let api_key = std::env::var("BRAVE_API_KEY").map_err(|_| {
                    ToolError::InvalidParams(
                        "BRAVE_API_KEY environment variable not set".to_string(),
                    )
                })?;
                let provider = BraveSearchProvider::new(&api_key, max_results)
                    .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
                provider
                    .search(query)
                    .await
                    .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?
            }
            SearchProviderConfig::Searxng { url } => {
                let provider = SearxngSearchProvider::new(url, max_results)
                    .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
                provider
                    .search(query)
                    .await
                    .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?
            }
        };

        // Cache results for reflection to curate later
        if let Err(e) = save_search_cache(&ctx.workspace, &ctx.session_id, query, &results) {
            logfire::warn!("failed to cache search results", error = e.to_string(),);
        }

        // Format results for the model
        let mut output = String::new();
        for (i, result) in results.iter().enumerate() {
            output.push_str(&format!("{}. {}\n", i + 1, result.title));
            output.push_str(&format!("   {}\n", result.url));
            if let Some(snippet) = &result.snippet {
                output.push_str(&format!("   {snippet}\n"));
            }
            if let Some(meta) = format_search_metadata(result) {
                output.push_str(&format!("   {meta}\n"));
            }
            output.push('\n');
        }

        if output.is_empty() {
            output = "No results found.".to_string();
        }

        Ok(output)
    }
}
