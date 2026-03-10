use super::output::ToolOutput;
use async_trait::async_trait;
use serde_json::{Value, json};

use crate::db;
use crate::db::knowledge::{
    SearchHit, find_topics_by_prefix, hybrid_merge, search_diary, search_notes, search_references,
    search_scripts,
};
use crate::embeddings::EmbeddingClient;
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
            description: "Search your knowledge base using hybrid BM25 + semantic \
                          search across notes, references, diary entries, and scripts. \
                          Search when the query relates to topics you may have notes, \
                          references, or diary entries about. Not needed for real-time \
                          queries or general knowledge. Defaults to notes and diary; \
                          pass categories to include references or scripts. Returns \
                          ranked results with snippets."
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
                            "enum": ["notes", "references", "diary", "topics", "scripts"]
                        },
                        "description": "Categories to search. Defaults to [\"notes\", \"diary\"]. Include \"references\" explicitly to search reference material. Use \"topics\" to search topic collections."
                    },
                    "topic": {
                        "type": "string",
                        "description": "Scope search to a topic name (e.g. \"dioxus\" or \"dioxus/docs\"). Prefix matching: \"dioxus\" matches all sub-topics. Only affects references and vector search."
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

    async fn execute(&self, params: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let query = params.get("query").and_then(Value::as_str).ok_or_else(|| {
            ToolError::InvalidParams("missing required parameter: query".to_string())
        })?;

        let limit = params
            .get("limit")
            .and_then(Value::as_u64)
            .map(|n| n as usize)
            .unwrap_or(DEFAULT_LIMIT);

        let topic = params.get("topic").and_then(Value::as_str);

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

        // When topic is set, auto-include references
        let use_defaults = categories.is_empty() && topic.is_none();
        let search_notes_flag = use_defaults || categories.iter().any(|c| c == "notes");
        let search_refs_flag =
            topic.is_some() || (!use_defaults && categories.iter().any(|c| c == "references"));
        let search_diary_flag = use_defaults || categories.iter().any(|c| c == "diary");
        let search_topics_flag = categories.iter().any(|c| c == "topics");
        let search_scripts_flag = !use_defaults && categories.iter().any(|c| c == "scripts");

        // Resolve topic name to topic IDs for scoped search
        let resolved_topic_ids = if let Some(topic_name) = topic {
            let topics = find_topics_by_prefix(&ctx.db, topic_name)
                .await
                .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
            topics.into_iter().map(|t| t.id).collect::<Vec<_>>()
        } else {
            vec![]
        };

        // Collect BM25 hits
        let mut bm25_hits = Vec::new();

        if search_notes_flag {
            bm25_hits.extend(
                search_notes(&ctx.db, query, limit)
                    .await
                    .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?,
            );
        }

        if search_refs_flag {
            if resolved_topic_ids.is_empty() && topic.is_none() {
                // No topic filter — search all references
                bm25_hits.extend(
                    search_references(&ctx.db, query, limit, None)
                        .await
                        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?,
                );
            } else {
                // Search references scoped to each matching topic
                for tid in &resolved_topic_ids {
                    bm25_hits.extend(
                        search_references(&ctx.db, query, limit, Some(tid))
                            .await
                            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?,
                    );
                }
            }
        }

        if search_diary_flag {
            bm25_hits.extend(
                search_diary(&ctx.db, query, limit)
                    .await
                    .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?,
            );
        }

        if search_topics_flag {
            bm25_hits.extend(
                db::knowledge::search_topics(&ctx.db, query, limit)
                    .await
                    .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?,
            );
        }

        if search_scripts_flag {
            bm25_hits.extend(
                search_scripts(&ctx.db, query, limit)
                    .await
                    .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?,
            );
        }

        // Build effective categories from computed flags so the vector filter
        // respects topic-triggered auto-include of references.
        let mut effective_categories = Vec::new();
        if search_notes_flag {
            effective_categories.push("notes".to_string());
        }
        if search_refs_flag {
            effective_categories.push("references".to_string());
        }
        if search_diary_flag {
            effective_categories.push("diary".to_string());
        }
        if search_topics_flag {
            effective_categories.push("topics".to_string());
        }
        if search_scripts_flag {
            effective_categories.push("scripts".to_string());
        }

        // Try hybrid search: embed query and merge with BM25
        let mut hits = try_hybrid_search(
            &ctx.config.embeddings,
            &ctx.db,
            bm25_hits,
            query,
            limit,
            &effective_categories,
            &resolved_topic_ids,
        )
        .await;

        // Enrich path for reference hits that came from vector-only merge
        for hit in &mut hits {
            if hit.kind == "reference"
                && hit.path.is_none()
                && let Ok(rec) = db::knowledge::get_reference(&ctx.db, &hit.id).await
            {
                hit.path = Some(format!("references/{}", rec.path));
            }
        }

        // Format output
        format_results(&hits).map(ToolOutput::text)
    }
}

/// Attempt hybrid BM25+vector search. Falls back to BM25-only if Ollama
/// is unavailable or embedding fails.
async fn try_hybrid_search(
    embeddings_config: &crate::config::EmbeddingsConfig,
    db: &crate::db::GhostDb,
    bm25_hits: Vec<SearchHit>,
    query: &str,
    limit: usize,
    categories: &[String],
    topic_ids: &[String],
) -> Vec<SearchHit> {
    let client = EmbeddingClient::new(embeddings_config);

    if !client.is_available().await {
        return fallback_bm25(bm25_hits, limit);
    }

    match client.embed_batch(&[query.to_string()]).await {
        Ok(vectors) if !vectors.is_empty() => {
            let embedding_hits =
                match db::embeddings::vector_search(db, &vectors[0], limit, topic_ids).await {
                    Ok(hits) => hits,
                    Err(e) => {
                        logfire::warn!(
                            "vector search failed, falling back to BM25",
                            error = e.to_string()
                        );
                        return fallback_bm25(bm25_hits, limit);
                    }
                };

            let filtered_hits = filter_embedding_hits(embedding_hits, categories);

            hybrid_merge(&bm25_hits, &filtered_hits, limit)
        }
        Ok(_) => {
            logfire::warn!("embedding returned empty vectors, falling back to BM25");
            fallback_bm25(bm25_hits, limit)
        }
        Err(e) => {
            logfire::warn!(
                "embedding query failed, falling back to BM25",
                error = e.to_string()
            );
            fallback_bm25(bm25_hits, limit)
        }
    }
}

/// Filter embedding hits to only include results from requested source
/// tables. Maps category names to source_table values:
/// "notes" → "note", "references" → "reference", "diary" → "diary".
fn filter_embedding_hits(
    hits: Vec<db::embeddings::EmbeddingHit>,
    categories: &[String],
) -> Vec<db::embeddings::EmbeddingHit> {
    if categories.is_empty() {
        // Default: notes + diary
        return hits
            .into_iter()
            .filter(|h| h.source_table == "note" || h.source_table == "diary")
            .collect();
    }

    let allowed_tables: Vec<&str> = categories
        .iter()
        .map(|c| match c.as_str() {
            "notes" => "note",
            "references" => "reference",
            "diary" => "diary",
            "scripts" => "script",
            other => other,
        })
        .collect();

    hits.into_iter()
        .filter(|h| allowed_tables.contains(&h.source_table.as_str()))
        .collect()
}

fn fallback_bm25(mut hits: Vec<SearchHit>, limit: usize) -> Vec<SearchHit> {
    hits.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    hits.truncate(limit);
    hits
}

fn format_results(hits: &[SearchHit]) -> Result<String, ToolError> {
    if hits.is_empty() {
        return Ok("No results found.".to_string());
    }

    let mut output = String::new();
    let mut current_kind: Option<&str> = None;

    // Group by kind for readability
    let mut sorted: Vec<&SearchHit> = hits.iter().collect();
    sorted.sort_by(|a, b| {
        a.kind.cmp(&b.kind).then(
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal),
        )
    });

    for hit in &sorted {
        let kind_header = match hit.kind.as_str() {
            "note" => "Notes",
            "reference" => "References",
            "diary" => "Diary",
            "script" => "Scripts",
            other => other,
        };

        if current_kind != Some(hit.kind.as_str()) {
            output.push_str(&format!("## {kind_header}\n\n"));
            current_kind = Some(&hit.kind);
        }

        if let Some(path) = &hit.path {
            output.push_str(&format!(
                "- **{}** (score: {:.2}, path: {})\n  {}\n\n",
                hit.title, hit.score, path, hit.snippet,
            ));
        } else {
            output.push_str(&format!(
                "- **{}** (id: {}, score: {:.2})\n  {}\n\n",
                hit.title, hit.id, hit.score, hit.snippet,
            ));
        }
    }

    output.push_str(&format!("---\n{} results total.", sorted.len()));
    Ok(output)
}
