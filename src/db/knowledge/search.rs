use std::collections::HashMap;

use sqlx::SqlitePool;

use crate::db::error::DatabaseError;

use super::records::{SearchHit, truncate_snippet};

const REFERENCE_FALLBACK_CONTENT_SCORE: f64 = 2.0;
const REFERENCE_FALLBACK_PATH_SCORE: f64 = 1.0;
const REFERENCE_FALLBACK_TOPIC_SCORE: f64 = 1.0;
const REFERENCE_FALLBACK_EXACT_QUERY_BONUS: f64 = 4.0;
const REFERENCE_FALLBACK_SNIPPET_LEN: usize = 500;

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
    match search_references_fts(db, query, limit, topic_id).await {
        Ok(hits) => Ok(hits),
        Err(error) if is_malformed_reference_fts_error(&error) => {
            tracing::warn!(
                query,
                topic_id,
                "reference_fts is malformed; falling back to plain reference search"
            );
            search_references_fallback(db, query, limit, topic_id).await
        }
        Err(error) => Err(error),
    }
}

async fn search_references_fts(
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

fn is_malformed_reference_fts_error(error: &DatabaseError) -> bool {
    match error {
        DatabaseError::Query {
            table,
            operation,
            source,
        } => {
            *table == "reference"
                && *operation == "search"
                && source
                    .to_string()
                    .contains("database disk image is malformed")
        }
        _ => false,
    }
}

async fn search_references_fallback(
    db: &SqlitePool,
    query: &str,
    limit: usize,
    topic_id: Option<&str>,
) -> Result<Vec<SearchHit>, DatabaseError> {
    let topic_names = fallback_topic_names(db).await?;
    let terms = fallback_query_terms(query);
    let query_lower = query.to_lowercase();
    let (min_rowid, max_rowid) = fallback_reference_rowid_bounds(db).await?;

    let mut hits = Vec::new();
    for rowid in min_rowid..=max_rowid {
        let row = match fetch_reference_metadata_for_fallback(db, rowid).await {
            Ok(Some(row)) => row,
            Ok(None) => continue,
            Err(error) if is_malformed_row_error(&error) => {
                tracing::warn!(rowid, "skipping corrupted reference metadata row");
                continue;
            }
            Err(error) => return Err(error),
        };

        if topic_id.is_some_and(|tid| tid != row.topic_id) {
            continue;
        }

        let topic_name = topic_names
            .get(&row.topic_id)
            .map(String::as_str)
            .unwrap_or(row.topic_id.as_str());
        let content = match fetch_reference_content_for_fallback(db, &row.id).await {
            Ok(content) => Some(content),
            Err(error) if is_malformed_row_error(&error) => {
                tracing::warn!(
                    ref_id = %row.id,
                    path = %row.path,
                    "skipping corrupted reference row during fallback search"
                );
                None
            }
            Err(error) => return Err(error),
        };

        let score = fallback_reference_score(
            topic_name,
            &row.path,
            content.as_deref(),
            &query_lower,
            &terms,
        );
        if score <= 0.0 {
            continue;
        }

        let snippet = match content {
            Some(content) => truncate_snippet(&content, REFERENCE_FALLBACK_SNIPPET_LEN),
            None => format!("Fallback match in references/{}", row.path),
        };

        hits.push(SearchHit {
            id: row.id,
            title: topic_name.to_string(),
            snippet,
            score,
            kind: "reference".to_string(),
            path: Some(format!("references/{}", row.path)),
        });
    }

    hits.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.path.cmp(&b.path))
    });
    hits.truncate(limit);
    Ok(hits)
}

async fn fallback_reference_rowid_bounds(db: &SqlitePool) -> Result<(i64, i64), DatabaseError> {
    let bounds: Option<(Option<i64>, Option<i64>)> =
        sqlx::query_as("SELECT MIN(rowid), MAX(rowid) FROM reference")
            .fetch_optional(db)
            .await
            .map_err(|source| DatabaseError::Query {
                table: "reference",
                operation: "search_fallback_bounds",
                source,
            })?;

    let (min_rowid, max_rowid) = bounds.unwrap_or((None, None));
    Ok((min_rowid.unwrap_or(1), max_rowid.unwrap_or(0)))
}

async fn fetch_reference_metadata_for_fallback(
    db: &SqlitePool,
    rowid: i64,
) -> Result<Option<FallbackReferenceMetadata>, DatabaseError> {
    sqlx::query_as::<_, FallbackReferenceMetadata>(
        "SELECT id, topic_id, path FROM reference WHERE rowid = ?",
    )
    .bind(rowid)
    .fetch_optional(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "reference",
        operation: "search_fallback",
        source,
    })
}

#[derive(sqlx::FromRow)]
struct FallbackReferenceMetadata {
    id: String,
    topic_id: String,
    path: String,
}

async fn fallback_topic_names(db: &SqlitePool) -> Result<HashMap<String, String>, DatabaseError> {
    #[derive(sqlx::FromRow)]
    struct TopicNameRow {
        id: String,
        name: String,
    }

    let rows = sqlx::query_as::<_, TopicNameRow>("SELECT id, name FROM topic")
        .fetch_all(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "topic",
            operation: "search_fallback_names",
            source,
        })?;

    Ok(rows.into_iter().map(|row| (row.id, row.name)).collect())
}

async fn fetch_reference_content_for_fallback(
    db: &SqlitePool,
    ref_id: &str,
) -> Result<String, DatabaseError> {
    let (content,): (String,) = sqlx::query_as("SELECT content FROM reference WHERE id = ?")
        .bind(ref_id)
        .fetch_one(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "reference",
            operation: "search_fallback_row",
            source,
        })?;

    Ok(content)
}

fn fallback_query_terms(query: &str) -> Vec<String> {
    let mut terms = Vec::new();

    for term in query.split_whitespace().map(str::to_lowercase) {
        if !terms.contains(&term) {
            terms.push(term);
        }
    }

    terms
}

fn fallback_reference_score(
    topic_name: &str,
    path: &str,
    content: Option<&str>,
    query_lower: &str,
    terms: &[String],
) -> f64 {
    let content_lower = content.map(str::to_lowercase);
    let path_lower = path.to_lowercase();
    let topic_lower = topic_name.to_lowercase();

    let mut score = 0.0;
    if !query_lower.is_empty()
        && (content_lower
            .as_ref()
            .is_some_and(|content| content.contains(query_lower))
            || path_lower.contains(query_lower)
            || topic_lower.contains(query_lower))
    {
        score += REFERENCE_FALLBACK_EXACT_QUERY_BONUS;
    }

    for term in terms {
        if content_lower
            .as_ref()
            .is_some_and(|content| content.contains(term))
        {
            score += REFERENCE_FALLBACK_CONTENT_SCORE;
        }
        if path_lower.contains(term) {
            score += REFERENCE_FALLBACK_PATH_SCORE;
        }
        if topic_lower.contains(term) {
            score += REFERENCE_FALLBACK_TOPIC_SCORE;
        }
    }

    score
}

fn is_malformed_row_error(error: &DatabaseError) -> bool {
    match error {
        DatabaseError::Query { source, .. } => source
            .to_string()
            .contains("database disk image is malformed"),
        _ => false,
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::knowledge::create_code_file;

    #[tokio::test]
    async fn search_code_files_filters_by_repo() {
        let dir = tempfile::TempDir::new().unwrap();
        let db = crate::db::connect(dir.path(), 384).await.unwrap();

        create_code_file(
            &db,
            "ghost",
            "src/main.rs",
            "fn main() { start_ghost() }",
            Some("a"),
        )
        .await
        .unwrap();
        create_code_file(&db, "other", "src/lib.rs", "fn start_ghost() {}", Some("b"))
            .await
            .unwrap();

        // Search with repo filter -- only ghost results
        let hits = search_code_files(&db, "ghost", 10, Some("ghost"))
            .await
            .unwrap();
        assert!(
            hits.iter()
                .all(|h| h.path.as_deref().unwrap().starts_with("code/ghost/"))
        );

        // Search without filter -- both repos
        let all = search_code_files(&db, "ghost", 10, None).await.unwrap();
        assert!(all.len() >= 2, "should find results from both repos");
    }
}
