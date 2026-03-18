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
    archetype: Option<&str>,
) -> Result<Vec<SearchHit>, DatabaseError> {
    #[derive(sqlx::FromRow)]
    struct NoteSearchRow {
        id: String,
        title: String,
        body: String,
        score: f64,
    }

    let fts_query = sanitize_fts_query(query);

    let rows = if let Some(arch) = archetype {
        sqlx::query_as::<_, NoteSearchRow>(
            "SELECT n.id, n.title, \
             snippet(note_fts, 1, '', '', '...', 80) AS body, \
             -bm25(note_fts, 2.0, 1.0) AS score \
             FROM note_fts \
             JOIN note n ON n.rowid = note_fts.rowid \
             WHERE note_fts MATCH ? AND n.archetype = ? \
             ORDER BY score DESC \
             LIMIT ?",
        )
        .bind(&fts_query)
        .bind(arch)
        .bind(limit as i64)
        .fetch_all(db)
        .await
    } else {
        sqlx::query_as::<_, NoteSearchRow>(
            "SELECT n.id, n.title, \
             snippet(note_fts, 1, '', '', '...', 80) AS body, \
             -bm25(note_fts, 2.0, 1.0) AS score \
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
    }
    .map_err(|source| DatabaseError::Query {
        table: "note",
        operation: "search",
        source,
    })?;

    Ok(rows
        .into_iter()
        .map(|r| {
            let snippet = truncate_snippet(&r.body, 500);
            SearchHit {
                id: r.id,
                title: r.title,
                snippet,
                score: r.score,
                kind: "note".to_string(),
                path: None,
            }
        })
        .collect())
}

#[tracing::instrument(skip_all, level = "debug", fields(query = %query))]
pub async fn search_references(
    db: &SqlitePool,
    query: &str,
    limit: usize,
    topic_id: Option<&str>,
) -> Result<Vec<SearchHit>, DatabaseError> {
    #[derive(sqlx::FromRow)]
    struct RefSearchRow {
        id: String,
        topic_name: String,
        path: String,
        snippet: String,
        score: f64,
    }

    let fts_query = sanitize_fts_query(query);

    let rows = if let Some(tid) = topic_id {
        sqlx::query_as::<_, RefSearchRow>(
            "SELECT r.id, COALESCE(t.name, r.topic_id) AS topic_name, r.path, \
             snippet(reference_fts, 0, '', '', '...', 80) AS snippet, \
             -bm25(reference_fts, 1.0) AS score \
             FROM reference_fts \
             JOIN reference r ON r.rowid = reference_fts.rowid \
             LEFT JOIN topic t ON t.id = r.topic_id \
             WHERE reference_fts MATCH ? AND r.topic_id = ? \
             ORDER BY score DESC \
             LIMIT ?",
        )
        .bind(&fts_query)
        .bind(tid)
        .bind(limit as i64)
        .fetch_all(db)
        .await
    } else {
        sqlx::query_as::<_, RefSearchRow>(
            "SELECT r.id, COALESCE(t.name, r.topic_id) AS topic_name, r.path, \
             snippet(reference_fts, 0, '', '', '...', 80) AS snippet, \
             -bm25(reference_fts, 1.0) AS score \
             FROM reference_fts \
             JOIN reference r ON r.rowid = reference_fts.rowid \
             LEFT JOIN topic t ON t.id = r.topic_id \
             WHERE reference_fts MATCH ? \
             ORDER BY score DESC \
             LIMIT ?",
        )
        .bind(&fts_query)
        .bind(limit as i64)
        .fetch_all(db)
        .await
    }
    .map_err(|source| DatabaseError::Query {
        table: "reference",
        operation: "search",
        source,
    })?;

    Ok(rows
        .into_iter()
        .map(|r| {
            let snippet = truncate_snippet(&r.snippet, 500);
            SearchHit {
                id: r.id,
                title: r.topic_name,
                snippet,
                score: r.score,
                kind: "reference".to_string(),
                path: Some(format!("references/{}", r.path)),
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
        "SELECT d.id, d.date, \
         snippet(diary_fts, 0, '', '', '...', 80) AS body, \
         -bm25(diary_fts) AS score \
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
            let snippet = truncate_snippet(&r.body, 500);
            SearchHit {
                id: r.id,
                title: r.date,
                snippet,
                score: r.score,
                kind: "diary".to_string(),
                path: None,
            }
        })
        .collect())
}

/// Full-text search topics by name using BM25 scoring.
#[tracing::instrument(skip_all, level = "debug", fields(query = %query))]
pub async fn search_topics(
    db: &SqlitePool,
    query: &str,
    limit: usize,
) -> Result<Vec<SearchHit>, DatabaseError> {
    #[derive(sqlx::FromRow)]
    struct TopicSearchRow {
        id: String,
        name: String,
        score: f64,
    }

    let fts_query = sanitize_fts_query(query);

    let rows = sqlx::query_as::<_, TopicSearchRow>(
        "SELECT t.id, t.name, -bm25(topic_fts) AS score \
         FROM topic_fts \
         JOIN topic t ON t.rowid = topic_fts.rowid \
         WHERE topic_fts MATCH ? \
         ORDER BY score DESC \
         LIMIT ?",
    )
    .bind(&fts_query)
    .bind(limit as i64)
    .fetch_all(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "topic",
        operation: "search",
        source,
    })?;

    Ok(rows
        .into_iter()
        .map(|r| SearchHit {
            id: r.id,
            title: r.name,
            snippet: String::new(),
            score: r.score,
            kind: "topic".to_string(),
            path: None,
        })
        .collect())
}

#[tracing::instrument(skip_all, level = "debug", fields(query = %query))]
pub async fn search_scripts(
    db: &SqlitePool,
    query: &str,
    limit: usize,
) -> Result<Vec<SearchHit>, DatabaseError> {
    #[derive(sqlx::FromRow)]
    struct ScriptSearchRow {
        id: String,
        path: String,
        content: String,
        score: f64,
    }

    let fts_query = sanitize_fts_query(query);

    let rows = sqlx::query_as::<_, ScriptSearchRow>(
        "SELECT s.id, s.path, \
         snippet(script_fts, 1, '', '', '...', 80) AS content, \
         -bm25(script_fts, 1.0, 1.0) AS score \
         FROM script_fts \
         JOIN script s ON s.rowid = script_fts.rowid \
         WHERE script_fts MATCH ? \
         ORDER BY score DESC \
         LIMIT ?",
    )
    .bind(&fts_query)
    .bind(limit as i64)
    .fetch_all(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "script",
        operation: "search",
        source,
    })?;

    Ok(rows
        .into_iter()
        .map(|r| {
            let snippet = truncate_snippet(&r.content, 500);
            SearchHit {
                id: r.id,
                title: r.path.clone(),
                snippet,
                score: r.score,
                kind: "script".to_string(),
                path: Some(format!("scripts/{}", r.path)),
            }
        })
        .collect())
}

#[tracing::instrument(skip_all, level = "debug", fields(query = %query))]
pub async fn search_code_files(
    db: &SqlitePool,
    query: &str,
    limit: usize,
    repo: Option<&str>,
) -> Result<Vec<SearchHit>, DatabaseError> {
    #[derive(sqlx::FromRow)]
    struct CodeSearchRow {
        id: String,
        repo: String,
        path: String,
        content: String,
        score: f64,
    }

    let fts_query = sanitize_fts_query(query);

    let rows = if let Some(repo_filter) = repo {
        sqlx::query_as::<_, CodeSearchRow>(
            "SELECT cf.id, cf.repo, cf.path, \
             snippet(code_file_fts, 2, '', '', '...', 80) AS content, \
             -bm25(code_file_fts, 1.0, 3.0, 1.0) AS score \
             FROM code_file_fts \
             JOIN code_file cf ON cf.rowid = code_file_fts.rowid \
             WHERE code_file_fts MATCH ? AND cf.repo = ? \
             ORDER BY score DESC \
             LIMIT ?",
        )
        .bind(&fts_query)
        .bind(repo_filter)
        .bind(limit as i64)
        .fetch_all(db)
        .await
    } else {
        sqlx::query_as::<_, CodeSearchRow>(
            "SELECT cf.id, cf.repo, cf.path, \
             snippet(code_file_fts, 2, '', '', '...', 80) AS content, \
             -bm25(code_file_fts, 1.0, 3.0, 1.0) AS score \
             FROM code_file_fts \
             JOIN code_file cf ON cf.rowid = code_file_fts.rowid \
             WHERE code_file_fts MATCH ? \
             ORDER BY score DESC \
             LIMIT ?",
        )
        .bind(&fts_query)
        .bind(limit as i64)
        .fetch_all(db)
        .await
    }
    .map_err(|source| DatabaseError::Query {
        table: "code_file",
        operation: "search",
        source,
    })?;

    Ok(rows
        .into_iter()
        .map(|r| {
            let snippet = truncate_snippet(&r.content, 500);
            SearchHit {
                id: r.id,
                title: format!("{}/{}", r.repo, r.path),
                snippet,
                score: r.score,
                kind: "code".to_string(),
                path: Some(format!("code/{}/{}", r.repo, r.path)),
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
            path: hit.path.clone(),
        });
        entry.score += 0.4 * normalized;
    }

    for hit in embedding_hits {
        let key = hit.source_id.clone();
        let chunk_snippet = truncate_snippet(&hit.chunk_text, 500);
        let entry = merged.entry(key).or_insert_with(|| SearchHit {
            id: hit.source_id.clone(),
            title: String::new(),
            snippet: chunk_snippet.clone(),
            score: 0.0,
            kind: hit.source_table.clone(),
            path: None,
        });
        entry.score += 0.6 * hit.score;
        // Prefer embedding chunk snippet — it's semantically matched to the query
        if !chunk_snippet.is_empty() {
            entry.snippet = chunk_snippet;
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
