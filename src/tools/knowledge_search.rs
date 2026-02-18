use async_trait::async_trait;
use serde_json::{Value, json};

use crate::db::knowledge::{search_diary, search_notes, search_references};
use crate::providers::ToolDefinition;

use super::context::ToolContext;
use super::error::ToolError;
use super::manager::Tool;

const DEFAULT_LIMIT: usize = 10;

#[derive(Debug)]
pub struct KnowledgeSearch;

#[async_trait]
impl Tool for KnowledgeSearch {
    fn name(&self) -> &str {
        "knowledge_search"
    }

    fn schema(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Search your knowledge base across notes, references, and \
                          diary entries. Use this FIRST before web search to check \
                          if you already have relevant information. Returns ranked \
                          results with snippets."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query string. Be specific."
                    },
                    "categories": {
                        "type": "array",
                        "items": {
                            "type": "string",
                            "enum": ["notes", "references", "diary"]
                        },
                        "description": "Categories to search. Omit to search all."
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Max results per category (default: 10)."
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

        let limit = params
            .get("limit")
            .and_then(Value::as_u64)
            .map(|n| n as usize)
            .unwrap_or(DEFAULT_LIMIT);

        let categories: Vec<String> = params
            .get("categories")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_str)
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default();

        let search_all = categories.is_empty();
        let search_notes_flag = search_all || categories.iter().any(|c| c == "notes");
        let search_refs_flag = search_all || categories.iter().any(|c| c == "references");
        let search_diary_flag = search_all || categories.iter().any(|c| c == "diary");

        let mut output = String::new();
        let mut total = 0usize;

        if search_notes_flag {
            let hits = search_notes(&ctx.db, query, limit)
                .await
                .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
            if !hits.is_empty() {
                output.push_str("## Notes\n\n");
                for hit in &hits {
                    output.push_str(&format!(
                        "- **{}** (id: {}, score: {:.2})\n  {}\n\n",
                        hit.title, hit.id, hit.score, hit.snippet,
                    ));
                }
                total += hits.len();
            }
        }

        if search_refs_flag {
            let hits = search_references(&ctx.db, query, limit)
                .await
                .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
            if !hits.is_empty() {
                output.push_str("## References\n\n");
                for hit in &hits {
                    output.push_str(&format!(
                        "- **{}** (id: {}, score: {:.2})\n  {}\n\n",
                        hit.title, hit.id, hit.score, hit.snippet,
                    ));
                }
                total += hits.len();
            }
        }

        if search_diary_flag {
            let hits = search_diary(&ctx.db, query, limit)
                .await
                .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
            if !hits.is_empty() {
                output.push_str("## Diary\n\n");
                for hit in &hits {
                    output.push_str(&format!(
                        "- **{}** (id: {}, score: {:.2})\n  {}\n\n",
                        hit.title, hit.id, hit.score, hit.snippet,
                    ));
                }
                total += hits.len();
            }
        }

        if output.is_empty() {
            output = "No results found.".to_string();
        } else {
            output.push_str(&format!("---\n{total} results total."));
        }

        Ok(output)
    }
}
