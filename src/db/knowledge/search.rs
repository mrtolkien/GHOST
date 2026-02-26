use std::collections::HashMap;

use serde::Deserialize;
use surrealdb::Surreal;
use surrealdb::engine::local::Db;
use surrealdb::types::{RecordId, SurrealValue};

use crate::db::error::DatabaseError;
use crate::db::query::{query_exec, take_many};

use super::records::{SearchHit, truncate_snippet};

#[derive(Debug, Deserialize, SurrealValue)]
#[surreal(crate = "surrealdb::types")]
struct NoteSearchRow {
    id: RecordId,
    title: String,
    body: String,
    score: f64,
}

#[derive(Debug, Deserialize, SurrealValue)]
#[surreal(crate = "surrealdb::types")]
struct RefSearchRow {
    id: RecordId,
    topic: String,
    content: String,
    score: f64,
}

#[derive(Debug, Deserialize, SurrealValue)]
#[surreal(crate = "surrealdb::types")]
struct DiarySearchRow {
    id: RecordId,
    date: String,
    body: String,
    score: f64,
}

/// Full-text search notes by title and body using BM25 scoring.
///
/// Title matches weighted 2x via `search::score(0) * 2`. Uses `@N@` +
/// `search::score(N)` (fixed in SurrealDB 3.1.0-alpha, see #6946).
#[tracing::instrument(skip_all, level = "debug", fields(query = %query))]
pub async fn search_notes(
    db: &Surreal<Db>,
    query: &str,
    limit: usize,
) -> Result<Vec<SearchHit>, DatabaseError> {
    let mut resp = query_exec(
        db.query(
            "SELECT id, title, body, \
                    search::score(0) * 2 + search::score(1) AS score \
             FROM note \
             WHERE title @0@ $query OR body @1@ $query \
             ORDER BY score DESC \
             LIMIT $limit",
        )
        .bind(("query", query.to_string()))
        .bind(("limit", limit as i64)),
        "note",
        "search",
    )
    .await?;
    let rows: Vec<NoteSearchRow> = take_many(&mut resp, 0, "note", "search")?;

    Ok(rows
        .into_iter()
        .map(|r| {
            let snippet = truncate_snippet(&r.body, 150);
            SearchHit {
                id: r.id,
                title: r.title,
                snippet,
                score: r.score,
                kind: "note".to_string(),
            }
        })
        .collect())
}

#[tracing::instrument(skip_all, level = "debug", fields(query = %query))]
pub async fn search_references(
    db: &Surreal<Db>,
    query: &str,
    limit: usize,
) -> Result<Vec<SearchHit>, DatabaseError> {
    let mut resp = query_exec(
        db.query(
            "SELECT id, topic, content, search::score(0) AS score \
             FROM reference \
             WHERE content @0@ $query \
             ORDER BY score DESC \
             LIMIT $limit",
        )
        .bind(("query", query.to_string()))
        .bind(("limit", limit as i64)),
        "reference",
        "search",
    )
    .await?;

    let rows: Vec<RefSearchRow> = take_many(&mut resp, 0, "reference", "search")?;
    Ok(rows
        .into_iter()
        .map(|r| {
            let snippet = truncate_snippet(&r.content, 150);
            SearchHit {
                id: r.id,
                title: r.topic,
                snippet,
                score: r.score,
                kind: "reference".to_string(),
            }
        })
        .collect())
}

#[tracing::instrument(skip_all, level = "debug", fields(query = %query))]
pub async fn search_diary(
    db: &Surreal<Db>,
    query: &str,
    limit: usize,
) -> Result<Vec<SearchHit>, DatabaseError> {
    let mut resp = query_exec(
        db.query(
            "SELECT id, date, body, search::score(0) AS score \
             FROM diary \
             WHERE body @0@ $query \
             ORDER BY score DESC \
             LIMIT $limit",
        )
        .bind(("query", query.to_string()))
        .bind(("limit", limit as i64)),
        "diary",
        "search",
    )
    .await?;

    let rows: Vec<DiarySearchRow> = take_many(&mut resp, 0, "diary", "search")?;
    Ok(rows
        .into_iter()
        .map(|r| {
            let snippet = truncate_snippet(&r.body, 150);
            SearchHit {
                id: r.id,
                title: r.date,
                snippet,
                score: r.score,
                kind: "diary".to_string(),
            }
        })
        .collect())
}

/// Merge BM25 results with vector search results using score fusion.
///
/// BM25 weight: 0.4, embedding weight: 0.6.
/// Results are merged by source ID — if a hit appears in both BM25 and
/// vector results, scores are combined.
pub fn hybrid_merge(
    bm25_hits: &[SearchHit],
    embedding_hits: &[crate::db::embeddings::EmbeddingHit],
    limit: usize,
) -> Vec<SearchHit> {
    let bm25_max = bm25_hits.iter().map(|h| h.score).fold(0.0_f64, f64::max);
    let bm25_max = if bm25_max > 0.0 { bm25_max } else { 1.0 };

    let mut merged: HashMap<String, SearchHit> = HashMap::new();

    for hit in bm25_hits {
        let key = crate::db::fmt_id(&hit.id);
        let normalized = hit.score / bm25_max;
        let entry = merged.entry(key).or_insert_with(|| SearchHit {
            id: hit.id.clone(),
            title: hit.title.clone(),
            snippet: hit.snippet.clone(),
            score: 0.0,
            kind: hit.kind.clone(),
        });
        entry.score += 0.4 * normalized;
    }

    for hit in embedding_hits {
        let key = crate::db::fmt_id(&hit.source_id);
        let entry = merged.entry(key).or_insert_with(|| SearchHit {
            id: hit.source_id.clone(),
            title: String::new(),
            snippet: truncate_snippet(&hit.chunk_text, 150),
            score: 0.0,
            kind: hit.source_table.clone(),
        });
        entry.score += 0.6 * hit.score;
        if entry.snippet.is_empty() {
            entry.snippet = truncate_snippet(&hit.chunk_text, 150);
        }
    }

    let mut results: Vec<SearchHit> = merged.into_values().collect();
    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    results.truncate(limit);
    results
}
