use serde::{Deserialize, Serialize};
use surrealdb::Surreal;
use surrealdb::engine::local::Db;
use surrealdb::sql::{Datetime, Thing};

use crate::db::error::DatabaseError;
use crate::db::query::{CountRow, IdRow, query_exec, take_many, take_one};

// --- Record types ---

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NoteRecord {
    pub id: Thing,
    pub title: String,
    pub body: String,
    pub archetype: Option<String>,
    pub tags: Vec<String>,
    pub trust: i64,
    pub created_at: Datetime,
    pub updated_at: Datetime,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReferenceRecord {
    pub id: Thing,
    pub topic: String,
    pub path: String,
    pub content: String,
    pub source_url: Option<String>,
    pub created_at: Datetime,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DiaryRecord {
    pub id: Thing,
    pub date: String,
    pub body: String,
    pub updated_at: Datetime,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchHit {
    pub id: Thing,
    pub title: String,
    pub snippet: String,
    pub score: f64,
    pub kind: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RecentItem {
    pub id: Thing,
    pub title: String,
    pub kind: String,
    pub updated_at: Datetime,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EdgeRecord {
    pub id: Thing,
    #[serde(rename = "in")]
    pub in_node: Thing,
    pub out: Thing,
    pub label: String,
    pub created_at: Datetime,
}

// --- Internal deserialization helpers ---

#[derive(Debug, Deserialize)]
struct OutRow {
    out: Thing,
}

#[derive(Debug, Deserialize)]
struct InRow {
    #[serde(rename = "in")]
    in_node: Thing,
}

#[derive(Debug, Deserialize)]
struct NoteSearchRow {
    id: Thing,
    title: String,
    body: String,
    score: f64,
}

#[derive(Debug, Deserialize)]
struct RefSearchRow {
    id: Thing,
    topic: String,
    content: String,
    score: f64,
}

#[derive(Debug, Deserialize)]
struct DiarySearchRow {
    id: Thing,
    date: String,
    body: String,
    score: f64,
}

// --- Create / Update ---

#[tracing::instrument(skip_all, level = "debug", fields(title = %title))]
pub async fn create_note(
    db: &Surreal<Db>,
    title: &str,
    body: &str,
) -> Result<Thing, DatabaseError> {
    create_note_full(db, title, body, None, &[], 5).await
}

#[tracing::instrument(skip_all, level = "debug", fields(title = %title))]
pub async fn create_note_full(
    db: &Surreal<Db>,
    title: &str,
    body: &str,
    archetype: Option<&str>,
    tags: &[String],
    trust: i64,
) -> Result<Thing, DatabaseError> {
    let mut resp = query_exec(
        db.query(
            "CREATE note SET \
                title = $title, \
                body = $body, \
                archetype = $archetype, \
                tags = $tags, \
                trust = $trust, \
                created_at = time::now(), \
                updated_at = time::now() \
             RETURN id",
        )
        .bind(("title", title.to_string()))
        .bind(("body", body.to_string()))
        .bind(("archetype", archetype.map(ToString::to_string)))
        .bind(("tags", tags.to_vec()))
        .bind(("trust", trust)),
        "note",
        "create",
    )
    .await?;

    let row: IdRow = take_one(&mut resp, 0, "note", "create")?;
    Ok(row.id)
}

#[tracing::instrument(skip_all, level = "debug", fields(note_id = %note_id))]
pub async fn update_note(
    db: &Surreal<Db>,
    note_id: &Thing,
    body: &str,
    archetype: Option<&str>,
    tags: &[String],
    trust: i64,
) -> Result<(), DatabaseError> {
    query_exec(
        db.query(
            "UPDATE $note_id SET \
                body = $body, \
                archetype = $archetype, \
                tags = $tags, \
                trust = $trust, \
                updated_at = time::now()",
        )
        .bind(("note_id", note_id.clone()))
        .bind(("body", body.to_string()))
        .bind(("archetype", archetype.map(ToString::to_string)))
        .bind(("tags", tags.to_vec()))
        .bind(("trust", trust)),
        "note",
        "update",
    )
    .await?;
    Ok(())
}

#[tracing::instrument(skip_all, level = "debug", fields(date = %date))]
pub async fn create_diary(
    db: &Surreal<Db>,
    date: &str,
    body: &str,
) -> Result<Thing, DatabaseError> {
    let mut resp = query_exec(
        db.query(
            "CREATE diary SET \
                date = $date, \
                body = $body, \
                updated_at = time::now() \
             RETURN id",
        )
        .bind(("date", date.to_string()))
        .bind(("body", body.to_string())),
        "diary",
        "create",
    )
    .await?;

    let row: IdRow = take_one(&mut resp, 0, "diary", "create")?;
    Ok(row.id)
}

#[tracing::instrument(skip_all, level = "debug", fields(date = %date))]
pub async fn append_diary(db: &Surreal<Db>, date: &str, line: &str) -> Result<(), DatabaseError> {
    query_exec(
        db.query(
            "UPDATE diary SET \
                body = string::concat(body, '\\n', $line), \
                updated_at = time::now() \
             WHERE date = $date",
        )
        .bind(("date", date.to_string()))
        .bind(("line", line.to_string())),
        "diary",
        "append",
    )
    .await?;
    Ok(())
}

#[tracing::instrument(skip_all, level = "debug", fields(topic = %topic, path = %path))]
pub async fn create_reference(
    db: &Surreal<Db>,
    topic: &str,
    path: &str,
    content: &str,
    source_url: Option<&str>,
) -> Result<Thing, DatabaseError> {
    let mut resp = query_exec(
        db.query(
            "CREATE reference SET \
                topic = $topic, \
                path = $path, \
                content = $content, \
                source_url = $source_url, \
                created_at = time::now() \
             RETURN id",
        )
        .bind(("topic", topic.to_string()))
        .bind(("path", path.to_string()))
        .bind(("content", content.to_string()))
        .bind(("source_url", source_url.map(ToString::to_string))),
        "reference",
        "create",
    )
    .await?;

    let row: IdRow = take_one(&mut resp, 0, "reference", "create")?;
    Ok(row.id)
}

// --- Read ---

#[tracing::instrument(skip_all, level = "debug", fields(note_id = %note_id))]
pub async fn get_note(db: &Surreal<Db>, note_id: &Thing) -> Result<NoteRecord, DatabaseError> {
    let mut resp = query_exec(
        db.query("SELECT * FROM ONLY $note_id")
            .bind(("note_id", note_id.clone())),
        "note",
        "get",
    )
    .await?;

    crate::db::query::take_opt(&mut resp, 0, "note", "get")?.ok_or(DatabaseError::MissingRow {
        table: "note",
        operation: "get",
    })
}

#[tracing::instrument(skip_all, level = "debug", fields(title = %title))]
pub async fn find_note_by_title(
    db: &Surreal<Db>,
    title: &str,
) -> Result<Option<NoteRecord>, DatabaseError> {
    let mut resp = query_exec(
        db.query("SELECT * FROM note WHERE title = $title LIMIT 1")
            .bind(("title", title.to_string())),
        "note",
        "find_by_title",
    )
    .await?;

    let rows: Vec<NoteRecord> = take_many(&mut resp, 0, "note", "find_by_title")?;
    Ok(rows.into_iter().next())
}

#[tracing::instrument(skip_all, level = "debug", fields(ref_id = %ref_id))]
pub async fn get_reference(
    db: &Surreal<Db>,
    ref_id: &Thing,
) -> Result<ReferenceRecord, DatabaseError> {
    let mut resp = query_exec(
        db.query("SELECT * FROM ONLY $ref_id")
            .bind(("ref_id", ref_id.clone())),
        "reference",
        "get",
    )
    .await?;

    crate::db::query::take_opt(&mut resp, 0, "reference", "get")?.ok_or(DatabaseError::MissingRow {
        table: "reference",
        operation: "get",
    })
}

#[tracing::instrument(skip_all, level = "debug", fields(date = %date))]
pub async fn get_diary_by_date(
    db: &Surreal<Db>,
    date: &str,
) -> Result<Option<DiaryRecord>, DatabaseError> {
    let mut resp = query_exec(
        db.query("SELECT * FROM diary WHERE date = $date LIMIT 1")
            .bind(("date", date.to_string())),
        "diary",
        "get_by_date",
    )
    .await?;

    let rows: Vec<DiaryRecord> = take_many(&mut resp, 0, "diary", "get_by_date")?;
    Ok(rows.into_iter().next())
}

#[tracing::instrument(skip_all, level = "debug")]
pub async fn list_recent(db: &Surreal<Db>, limit: usize) -> Result<Vec<RecentItem>, DatabaseError> {
    let mut resp = query_exec(
        db.query(
            "SELECT id, title, 'note' AS kind, updated_at FROM note \
             ORDER BY updated_at DESC LIMIT $limit; \
             SELECT id, topic AS title, 'reference' AS kind, created_at AS updated_at FROM reference \
             ORDER BY updated_at DESC LIMIT $limit; \
             SELECT id, date AS title, 'diary' AS kind, updated_at FROM diary \
             ORDER BY updated_at DESC LIMIT $limit;",
        )
        .bind(("limit", limit as i64)),
        "knowledge",
        "list_recent",
    )
    .await?;

    let notes: Vec<RecentItem> = take_many(&mut resp, 0, "knowledge", "list_recent/notes")?;
    let refs: Vec<RecentItem> = take_many(&mut resp, 1, "knowledge", "list_recent/refs")?;
    let diary: Vec<RecentItem> = take_many(&mut resp, 2, "knowledge", "list_recent/diary")?;

    let mut all = notes;
    all.extend(refs);
    all.extend(diary);
    all.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    all.truncate(limit);
    Ok(all)
}

// --- Delete ---

#[tracing::instrument(skip_all, level = "debug", fields(note_id = %note_id))]
pub async fn delete_note(db: &Surreal<Db>, note_id: &Thing) -> Result<(), DatabaseError> {
    query_exec(
        db.query(
            "DELETE relates_to WHERE `in` = $note_id OR out = $note_id; \
             DELETE cited WHERE `in` = $note_id OR out = $note_id; \
             DELETE $note_id",
        )
        .bind(("note_id", note_id.clone())),
        "note",
        "delete",
    )
    .await?;
    Ok(())
}

#[tracing::instrument(skip_all, level = "debug", fields(ref_id = %ref_id))]
pub async fn delete_reference(db: &Surreal<Db>, ref_id: &Thing) -> Result<(), DatabaseError> {
    query_exec(
        db.query(
            "DELETE cited WHERE `in` = $ref_id OR out = $ref_id; \
             DELETE $ref_id",
        )
        .bind(("ref_id", ref_id.clone())),
        "reference",
        "delete",
    )
    .await?;
    Ok(())
}

// --- Search (BM25) ---

#[tracing::instrument(skip_all, level = "debug", fields(query = %query))]
pub async fn search_notes(
    db: &Surreal<Db>,
    query: &str,
    limit: usize,
) -> Result<Vec<SearchHit>, DatabaseError> {
    let mut resp = query_exec(
        db.query(
            "SELECT id, title, body, \
                search::score(0) + search::score(1) AS score \
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
            "SELECT id, topic, content, \
                search::score(0) AS score \
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
            "SELECT id, date, body, \
                search::score(0) AS score \
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

// --- Hybrid search ---

/// Merge BM25 results with vector search results using score fusion.
///
/// BM25 weight: 0.4, embedding weight: 0.6.
/// Results are merged by source ID — if a hit appears in both BM25 and
/// vector results, scores are combined.
pub fn hybrid_merge(
    bm25_hits: &[SearchHit],
    embedding_hits: &[super::embeddings::EmbeddingHit],
    limit: usize,
) -> Vec<SearchHit> {
    use std::collections::HashMap;

    // Normalize BM25 scores to 0..1
    let bm25_max = bm25_hits.iter().map(|h| h.score).fold(0.0_f64, f64::max);
    let bm25_max = if bm25_max > 0.0 { bm25_max } else { 1.0 };

    let mut merged: HashMap<String, SearchHit> = HashMap::new();

    for hit in bm25_hits {
        let key = hit.id.to_string();
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
        let key = hit.source_id.to_string();
        let entry = merged.entry(key).or_insert_with(|| SearchHit {
            id: hit.source_id.clone(),
            title: String::new(),
            snippet: truncate_snippet(&hit.chunk_text, 150),
            score: 0.0,
            kind: hit.source_table.clone(),
        });
        // Cosine similarity is already 0..1
        entry.score += 0.6 * hit.score;
        // Fill in snippet from embedding chunk if BM25 didn't provide one
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

// --- Graph ---

#[tracing::instrument(skip_all, level = "debug", fields(from = %from, to = %to, label = %label))]
pub async fn create_edge(
    db: &Surreal<Db>,
    from: &Thing,
    to: &Thing,
    label: &str,
) -> Result<Thing, DatabaseError> {
    let mut resp = query_exec(
        db.query(
            "RELATE $from->relates_to->$to SET label = $label, created_at = time::now() RETURN id",
        )
        .bind(("from", from.clone()))
        .bind(("to", to.clone()))
        .bind(("label", label.to_string())),
        "relates_to",
        "create_edge",
    )
    .await?;

    let row: IdRow = take_one(&mut resp, 0, "relates_to", "create_edge")?;
    Ok(row.id)
}

#[tracing::instrument(skip_all, level = "debug", fields(from = %from))]
pub async fn related_note_ids(db: &Surreal<Db>, from: &Thing) -> Result<Vec<Thing>, DatabaseError> {
    let mut resp = query_exec(
        db.query("SELECT out FROM relates_to WHERE `in` = $from")
            .bind(("from", from.clone())),
        "relates_to",
        "related_note_ids",
    )
    .await?;

    let rows: Vec<OutRow> = take_many(&mut resp, 0, "relates_to", "related_note_ids")?;
    Ok(rows.into_iter().map(|row| row.out).collect())
}

#[tracing::instrument(skip_all, level = "debug", fields(note_id = %note_id))]
pub async fn outgoing_edges(
    db: &Surreal<Db>,
    note_id: &Thing,
) -> Result<Vec<EdgeRecord>, DatabaseError> {
    let mut resp = query_exec(
        db.query("SELECT * FROM relates_to WHERE `in` = $note_id")
            .bind(("note_id", note_id.clone())),
        "relates_to",
        "outgoing_edges",
    )
    .await?;

    take_many(&mut resp, 0, "relates_to", "outgoing_edges")
}

#[tracing::instrument(skip_all, level = "debug", fields(note_id = %note_id))]
pub async fn incoming_edges(
    db: &Surreal<Db>,
    note_id: &Thing,
) -> Result<Vec<EdgeRecord>, DatabaseError> {
    let mut resp = query_exec(
        db.query("SELECT * FROM relates_to WHERE out = $note_id")
            .bind(("note_id", note_id.clone())),
        "relates_to",
        "incoming_edges",
    )
    .await?;

    take_many(&mut resp, 0, "relates_to", "incoming_edges")
}

#[tracing::instrument(skip_all, level = "debug", fields(note_id = %note_id))]
pub async fn incoming_cited(
    db: &Surreal<Db>,
    note_id: &Thing,
) -> Result<Vec<Thing>, DatabaseError> {
    let mut resp = query_exec(
        db.query("SELECT `in` FROM cited WHERE out = $note_id")
            .bind(("note_id", note_id.clone())),
        "cited",
        "incoming_cited",
    )
    .await?;

    let rows: Vec<InRow> = take_many(&mut resp, 0, "cited", "incoming_cited")?;
    Ok(rows.into_iter().map(|row| row.in_node).collect())
}

#[tracing::instrument(skip_all, level = "debug", fields(note_id = %note_id))]
pub async fn delete_outgoing_edges(db: &Surreal<Db>, note_id: &Thing) -> Result<(), DatabaseError> {
    query_exec(
        db.query("DELETE relates_to WHERE `in` = $note_id")
            .bind(("note_id", note_id.clone())),
        "relates_to",
        "delete_outgoing",
    )
    .await?;
    Ok(())
}

#[tracing::instrument(skip_all, level = "debug")]
pub async fn orphan_notes(db: &Surreal<Db>) -> Result<Vec<NoteRecord>, DatabaseError> {
    let mut resp = query_exec(
        db.query(
            "SELECT * FROM note WHERE \
             count(->relates_to) = 0 AND \
             count(<-relates_to) = 0",
        ),
        "note",
        "orphan_notes",
    )
    .await?;

    take_many(&mut resp, 0, "note", "orphan_notes")
}

// --- Stats / Tags ---

#[tracing::instrument(skip_all, level = "debug")]
pub async fn count_notes(db: &Surreal<Db>) -> Result<i64, DatabaseError> {
    count_table(db, "note").await
}

#[tracing::instrument(skip_all, level = "debug")]
pub async fn count_references(db: &Surreal<Db>) -> Result<i64, DatabaseError> {
    count_table(db, "reference").await
}

#[tracing::instrument(skip_all, level = "debug")]
pub async fn count_diary(db: &Surreal<Db>) -> Result<i64, DatabaseError> {
    count_table(db, "diary").await
}

#[tracing::instrument(skip_all, level = "debug")]
pub async fn count_edges(db: &Surreal<Db>) -> Result<i64, DatabaseError> {
    count_table(db, "relates_to").await
}

#[tracing::instrument(skip_all, level = "debug")]
pub async fn count_stubs(db: &Surreal<Db>) -> Result<i64, DatabaseError> {
    let mut resp = query_exec(
        db.query("SELECT count() AS count FROM note WHERE body = '' AND trust = 1 GROUP ALL"),
        "note",
        "count_stubs",
    )
    .await?;

    let rows: Vec<CountRow> = take_many(&mut resp, 0, "note", "count_stubs")?;
    Ok(rows.first().map_or(0, |r| r.count))
}

#[derive(Debug, Deserialize)]
struct TagCountRow {
    tags: String,
    count: i64,
}

#[tracing::instrument(skip_all, level = "debug")]
pub async fn list_tags_with_counts(db: &Surreal<Db>) -> Result<Vec<(String, i64)>, DatabaseError> {
    let mut resp = query_exec(
        db.query(
            "SELECT tags, count() AS count \
             FROM (SELECT * FROM note SPLIT tags) \
             WHERE tags IS NOT NONE \
             GROUP BY tags \
             ORDER BY count DESC",
        ),
        "note",
        "list_tags",
    )
    .await?;

    let rows: Vec<TagCountRow> = take_many(&mut resp, 0, "note", "list_tags")?;
    Ok(rows.into_iter().map(|r| (r.tags, r.count)).collect())
}

// --- Reference updates ---

#[tracing::instrument(skip_all, level = "debug", fields(ref_id = %ref_id))]
pub async fn update_reference_path(
    db: &Surreal<Db>,
    ref_id: &Thing,
    new_path: &str,
    new_topic: &str,
) -> Result<(), DatabaseError> {
    query_exec(
        db.query("UPDATE $ref_id SET path = $path, topic = $topic")
            .bind(("ref_id", ref_id.clone()))
            .bind(("path", new_path.to_string()))
            .bind(("topic", new_topic.to_string())),
        "reference",
        "update_path",
    )
    .await?;
    Ok(())
}

#[tracing::instrument(skip_all, level = "debug", fields(path = %path))]
pub async fn find_reference_by_path(
    db: &Surreal<Db>,
    path: &str,
) -> Result<Option<ReferenceRecord>, DatabaseError> {
    let mut resp = query_exec(
        db.query("SELECT * FROM reference WHERE path = $path LIMIT 1")
            .bind(("path", path.to_string())),
        "reference",
        "find_by_path",
    )
    .await?;

    let rows: Vec<ReferenceRecord> = take_many(&mut resp, 0, "reference", "find_by_path")?;
    Ok(rows.into_iter().next())
}

// --- Bulk listing for embeddings pipeline ---

pub async fn list_all_notes(db: &Surreal<Db>) -> Result<Vec<NoteRecord>, DatabaseError> {
    let mut resp = query_exec(db.query("SELECT * FROM note"), "note", "list_all").await?;
    take_many(&mut resp, 0, "note", "list_all")
}

pub async fn list_all_references(db: &Surreal<Db>) -> Result<Vec<ReferenceRecord>, DatabaseError> {
    let mut resp = query_exec(db.query("SELECT * FROM reference"), "reference", "list_all").await?;
    take_many(&mut resp, 0, "reference", "list_all")
}

pub async fn list_all_diary(db: &Surreal<Db>) -> Result<Vec<DiaryRecord>, DatabaseError> {
    let mut resp = query_exec(db.query("SELECT * FROM diary"), "diary", "list_all").await?;
    take_many(&mut resp, 0, "diary", "list_all")
}

// --- Helpers ---

async fn count_table(db: &Surreal<Db>, table: &'static str) -> Result<i64, DatabaseError> {
    let query = format!("SELECT count() AS count FROM {table} GROUP ALL");
    let mut resp = query_exec(db.query(&query), table, "count").await?;
    let rows: Vec<CountRow> = take_many(&mut resp, 0, table, "count")?;
    Ok(rows.first().map_or(0, |r| r.count))
}

fn truncate_snippet(text: &str, max_len: usize) -> String {
    let first_line = text.lines().next().unwrap_or("");
    if first_line.len() <= max_len {
        first_line.to_string()
    } else {
        format!("{}...", &first_line[..max_len])
    }
}
