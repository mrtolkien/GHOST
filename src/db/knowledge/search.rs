use std::collections::HashMap;

use sqlx::SqlitePool;

use crate::db::error::DatabaseError;

use super::records::{SearchHit, truncate_snippet};

/// Sanitize user input for FTS5 MATCH queries by quoting each term.
fn sanitize_fts_query(query: &str) -> String {
    query
        .split_whitespace()
        .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Full-text search notes by title and body using BM25 scoring.
///
/// Title matches weighted 2x via `bm25(note_fts, 2.0, 1.0)`.
/// FTS5 bm25() returns negative scores; we negate at the boundary.
#[tracing::instrument(skip_all, level = "debug", fields(query = %query))]
pub async fn search_notes(
    db: &SqlitePool,
    query: &str,
    limit: usize,
) -> Result<Vec<SearchHit>, DatabaseError> {
    #[derive(sqlx::FromRow)]
    struct NoteSearchRow {
        id: String,
        title: String,
        body: String,
        score: f64,
    }

    let fts_query = sanitize_fts_query(query);

    let rows = sqlx::query_as::<_, NoteSearchRow>(
        "SELECT n.id, n.title, n.body, -bm25(note_fts, 2.0, 1.0) AS score \
         FROM note_fts \
         JOIN note n ON n.rowid = note_fts.rowid \
         WHERE note_fts MATCH ? \
         ORDER BY score DESC \
         LIMIT ?",
    )
    .bind(&fts_query)
    .bind(limit as i64)
    .fetch_all(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "note",
        operation: "search",
        source,
    })?;

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
    db: &SqlitePool,
    query: &str,
    limit: usize,
) -> Result<Vec<SearchHit>, DatabaseError> {
    #[derive(sqlx::FromRow)]
    struct RefSearchRow {
        id: String,
        topic: String,
        content: String,
        score: f64,
    }

    let fts_query = sanitize_fts_query(query);

    let rows = sqlx::query_as::<_, RefSearchRow>(
        "SELECT r.id, r.topic, r.content, -bm25(reference_fts) AS score \
         FROM reference_fts \
         JOIN reference r ON r.rowid = reference_fts.rowid \
         WHERE reference_fts MATCH ? \
         ORDER BY score DESC \
         LIMIT ?",
    )
    .bind(&fts_query)
    .bind(limit as i64)
    .fetch_all(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "reference",
        operation: "search",
        source,
    })?;

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
    db: &SqlitePool,
    query: &str,
    limit: usize,
) -> Result<Vec<SearchHit>, DatabaseError> {
    #[derive(sqlx::FromRow)]
    struct DiarySearchRow {
        id: String,
        date: String,
        body: String,
        score: f64,
    }

    let fts_query = sanitize_fts_query(query);

    let rows = sqlx::query_as::<_, DiarySearchRow>(
        "SELECT d.id, d.date, d.body, -bm25(diary_fts) AS score \
         FROM diary_fts \
         JOIN diary d ON d.rowid = diary_fts.rowid \
         WHERE diary_fts MATCH ? \
         ORDER BY score DESC \
         LIMIT ?",
    )
    .bind(&fts_query)
    .bind(limit as i64)
    .fetch_all(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "diary",
        operation: "search",
        source,
    })?;

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
        let key = hit.id.clone();
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
        let key = hit.source_id.clone();
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
