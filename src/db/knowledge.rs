use serde::{Deserialize, Serialize};
use surrealdb::Surreal;
use surrealdb::engine::local::Db;
use surrealdb::sql::{Datetime, Thing};

use crate::db::error::DatabaseError;

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
struct IdRow {
    id: Thing,
}

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
struct CountRow {
    count: i64,
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
    let mut response = db
        .query(
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
        .bind(("trust", trust))
        .await
        .map_err(|source| DatabaseError::Query {
            table: "note",
            operation: "create",
            source,
        })?;

    let rows: Vec<IdRow> = response.take(0).map_err(|source| DatabaseError::Query {
        table: "note",
        operation: "create/take",
        source,
    })?;

    rows.into_iter()
        .next()
        .map(|row| row.id)
        .ok_or(DatabaseError::MissingRow {
            table: "note",
            operation: "create",
        })
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
    .bind(("trust", trust))
    .await
    .map_err(|source| DatabaseError::Query {
        table: "note",
        operation: "update",
        source,
    })?;
    Ok(())
}

#[tracing::instrument(skip_all, level = "debug", fields(date = %date))]
pub async fn create_diary(
    db: &Surreal<Db>,
    date: &str,
    body: &str,
) -> Result<Thing, DatabaseError> {
    let mut response = db
        .query(
            "CREATE diary SET \
                date = $date, \
                body = $body, \
                updated_at = time::now() \
             RETURN id",
        )
        .bind(("date", date.to_string()))
        .bind(("body", body.to_string()))
        .await
        .map_err(|source| DatabaseError::Query {
            table: "diary",
            operation: "create",
            source,
        })?;

    let rows: Vec<IdRow> = response.take(0).map_err(|source| DatabaseError::Query {
        table: "diary",
        operation: "create/take",
        source,
    })?;

    rows.into_iter()
        .next()
        .map(|row| row.id)
        .ok_or(DatabaseError::MissingRow {
            table: "diary",
            operation: "create",
        })
}

#[tracing::instrument(skip_all, level = "debug", fields(date = %date))]
pub async fn append_diary(db: &Surreal<Db>, date: &str, line: &str) -> Result<(), DatabaseError> {
    db.query(
        "UPDATE diary SET \
            body = string::concat(body, '\\n', $line), \
            updated_at = time::now() \
         WHERE date = $date",
    )
    .bind(("date", date.to_string()))
    .bind(("line", line.to_string()))
    .await
    .map_err(|source| DatabaseError::Query {
        table: "diary",
        operation: "append",
        source,
    })?;
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
    let mut response = db
        .query(
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
        .bind(("source_url", source_url.map(ToString::to_string)))
        .await
        .map_err(|source| DatabaseError::Query {
            table: "reference",
            operation: "create",
            source,
        })?;

    let rows: Vec<IdRow> = response.take(0).map_err(|source| DatabaseError::Query {
        table: "reference",
        operation: "create/take",
        source,
    })?;

    rows.into_iter()
        .next()
        .map(|row| row.id)
        .ok_or(DatabaseError::MissingRow {
            table: "reference",
            operation: "create",
        })
}

// --- Read ---

#[tracing::instrument(skip_all, level = "debug", fields(note_id = %note_id))]
pub async fn get_note(db: &Surreal<Db>, note_id: &Thing) -> Result<NoteRecord, DatabaseError> {
    let mut response = db
        .query("SELECT * FROM ONLY $note_id")
        .bind(("note_id", note_id.clone()))
        .await
        .map_err(|source| DatabaseError::Query {
            table: "note",
            operation: "get",
            source,
        })?;

    response
        .take::<Option<NoteRecord>>(0)
        .map_err(|source| DatabaseError::Query {
            table: "note",
            operation: "get/take",
            source,
        })?
        .ok_or(DatabaseError::MissingRow {
            table: "note",
            operation: "get",
        })
}

#[tracing::instrument(skip_all, level = "debug", fields(title = %title))]
pub async fn find_note_by_title(
    db: &Surreal<Db>,
    title: &str,
) -> Result<Option<NoteRecord>, DatabaseError> {
    let mut response = db
        .query("SELECT * FROM note WHERE title = $title LIMIT 1")
        .bind(("title", title.to_string()))
        .await
        .map_err(|source| DatabaseError::Query {
            table: "note",
            operation: "find_by_title",
            source,
        })?;

    let rows: Vec<NoteRecord> = response.take(0).map_err(|source| DatabaseError::Query {
        table: "note",
        operation: "find_by_title/take",
        source,
    })?;

    Ok(rows.into_iter().next())
}

#[tracing::instrument(skip_all, level = "debug", fields(ref_id = %ref_id))]
pub async fn get_reference(
    db: &Surreal<Db>,
    ref_id: &Thing,
) -> Result<ReferenceRecord, DatabaseError> {
    let mut response = db
        .query("SELECT * FROM ONLY $ref_id")
        .bind(("ref_id", ref_id.clone()))
        .await
        .map_err(|source| DatabaseError::Query {
            table: "reference",
            operation: "get",
            source,
        })?;

    response
        .take::<Option<ReferenceRecord>>(0)
        .map_err(|source| DatabaseError::Query {
            table: "reference",
            operation: "get/take",
            source,
        })?
        .ok_or(DatabaseError::MissingRow {
            table: "reference",
            operation: "get",
        })
}

#[tracing::instrument(skip_all, level = "debug", fields(date = %date))]
pub async fn get_diary_by_date(
    db: &Surreal<Db>,
    date: &str,
) -> Result<Option<DiaryRecord>, DatabaseError> {
    let mut response = db
        .query("SELECT * FROM diary WHERE date = $date LIMIT 1")
        .bind(("date", date.to_string()))
        .await
        .map_err(|source| DatabaseError::Query {
            table: "diary",
            operation: "get_by_date",
            source,
        })?;

    let rows: Vec<DiaryRecord> = response.take(0).map_err(|source| DatabaseError::Query {
        table: "diary",
        operation: "get_by_date/take",
        source,
    })?;

    Ok(rows.into_iter().next())
}

#[tracing::instrument(skip_all, level = "debug")]
pub async fn list_recent(db: &Surreal<Db>, limit: usize) -> Result<Vec<RecentItem>, DatabaseError> {
    let mut response = db
        .query(
            "SELECT id, title, 'note' AS kind, updated_at FROM note \
             ORDER BY updated_at DESC LIMIT $limit; \
             SELECT id, topic AS title, 'reference' AS kind, created_at AS updated_at FROM reference \
             ORDER BY updated_at DESC LIMIT $limit; \
             SELECT id, date AS title, 'diary' AS kind, updated_at FROM diary \
             ORDER BY updated_at DESC LIMIT $limit;",
        )
        .bind(("limit", limit as i64))
        .await
        .map_err(|source| DatabaseError::Query {
            table: "knowledge",
            operation: "list_recent",
            source,
        })?;

    let notes: Vec<RecentItem> = response.take(0).map_err(|source| DatabaseError::Query {
        table: "knowledge",
        operation: "list_recent/notes",
        source,
    })?;
    let refs: Vec<RecentItem> = response.take(1).map_err(|source| DatabaseError::Query {
        table: "knowledge",
        operation: "list_recent/refs",
        source,
    })?;
    let diary: Vec<RecentItem> = response.take(2).map_err(|source| DatabaseError::Query {
        table: "knowledge",
        operation: "list_recent/diary",
        source,
    })?;

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
    db.query(
        "DELETE relates_to WHERE `in` = $note_id OR out = $note_id; \
         DELETE cited WHERE `in` = $note_id OR out = $note_id; \
         DELETE $note_id",
    )
    .bind(("note_id", note_id.clone()))
    .await
    .map_err(|source| DatabaseError::Query {
        table: "note",
        operation: "delete",
        source,
    })?;
    Ok(())
}

#[tracing::instrument(skip_all, level = "debug", fields(ref_id = %ref_id))]
pub async fn delete_reference(db: &Surreal<Db>, ref_id: &Thing) -> Result<(), DatabaseError> {
    db.query(
        "DELETE cited WHERE `in` = $ref_id OR out = $ref_id; \
         DELETE $ref_id",
    )
    .bind(("ref_id", ref_id.clone()))
    .await
    .map_err(|source| DatabaseError::Query {
        table: "reference",
        operation: "delete",
        source,
    })?;
    Ok(())
}

// --- Search (BM25) ---

#[tracing::instrument(skip_all, level = "debug", fields(query = %query))]
pub async fn search_notes(
    db: &Surreal<Db>,
    query: &str,
    limit: usize,
) -> Result<Vec<SearchHit>, DatabaseError> {
    let mut response = db
        .query(
            "SELECT id, title, body, \
                search::score(0) + search::score(1) AS score \
             FROM note \
             WHERE title @0@ $query OR body @1@ $query \
             ORDER BY score DESC \
             LIMIT $limit",
        )
        .bind(("query", query.to_string()))
        .bind(("limit", limit as i64))
        .await
        .map_err(|source| DatabaseError::Query {
            table: "note",
            operation: "search",
            source,
        })?;

    let rows: Vec<NoteSearchRow> = response.take(0).map_err(|source| DatabaseError::Query {
        table: "note",
        operation: "search/take",
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
    db: &Surreal<Db>,
    query: &str,
    limit: usize,
) -> Result<Vec<SearchHit>, DatabaseError> {
    let mut response = db
        .query(
            "SELECT id, topic, content, \
                search::score(0) AS score \
             FROM reference \
             WHERE content @0@ $query \
             ORDER BY score DESC \
             LIMIT $limit",
        )
        .bind(("query", query.to_string()))
        .bind(("limit", limit as i64))
        .await
        .map_err(|source| DatabaseError::Query {
            table: "reference",
            operation: "search",
            source,
        })?;

    let rows: Vec<RefSearchRow> = response.take(0).map_err(|source| DatabaseError::Query {
        table: "reference",
        operation: "search/take",
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
    db: &Surreal<Db>,
    query: &str,
    limit: usize,
) -> Result<Vec<SearchHit>, DatabaseError> {
    let mut response = db
        .query(
            "SELECT id, date, body, \
                search::score(0) AS score \
             FROM diary \
             WHERE body @0@ $query \
             ORDER BY score DESC \
             LIMIT $limit",
        )
        .bind(("query", query.to_string()))
        .bind(("limit", limit as i64))
        .await
        .map_err(|source| DatabaseError::Query {
            table: "diary",
            operation: "search",
            source,
        })?;

    let rows: Vec<DiarySearchRow> = response.take(0).map_err(|source| DatabaseError::Query {
        table: "diary",
        operation: "search/take",
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

// --- Graph ---

#[tracing::instrument(skip_all, level = "debug", fields(from = %from, to = %to, label = %label))]
pub async fn create_edge(
    db: &Surreal<Db>,
    from: &Thing,
    to: &Thing,
    label: &str,
) -> Result<Thing, DatabaseError> {
    let mut response = db
        .query(
            "RELATE $from->relates_to->$to SET label = $label, created_at = time::now() RETURN id",
        )
        .bind(("from", from.clone()))
        .bind(("to", to.clone()))
        .bind(("label", label.to_string()))
        .await
        .map_err(|source| DatabaseError::Query {
            table: "relates_to",
            operation: "create_edge",
            source,
        })?;

    let rows: Vec<IdRow> = response.take(0).map_err(|source| DatabaseError::Query {
        table: "relates_to",
        operation: "create_edge/take",
        source,
    })?;

    rows.into_iter()
        .next()
        .map(|row| row.id)
        .ok_or(DatabaseError::MissingRow {
            table: "relates_to",
            operation: "create_edge",
        })
}

#[tracing::instrument(skip_all, level = "debug", fields(from = %from))]
pub async fn related_note_ids(db: &Surreal<Db>, from: &Thing) -> Result<Vec<Thing>, DatabaseError> {
    let mut response = db
        .query("SELECT out FROM relates_to WHERE `in` = $from")
        .bind(("from", from.clone()))
        .await
        .map_err(|source| DatabaseError::Query {
            table: "relates_to",
            operation: "related_note_ids",
            source,
        })?;

    let rows: Vec<OutRow> = response.take(0).map_err(|source| DatabaseError::Query {
        table: "relates_to",
        operation: "related_note_ids/take",
        source,
    })?;

    Ok(rows.into_iter().map(|row| row.out).collect())
}

#[tracing::instrument(skip_all, level = "debug", fields(note_id = %note_id))]
pub async fn outgoing_edges(
    db: &Surreal<Db>,
    note_id: &Thing,
) -> Result<Vec<EdgeRecord>, DatabaseError> {
    let mut response = db
        .query("SELECT * FROM relates_to WHERE `in` = $note_id")
        .bind(("note_id", note_id.clone()))
        .await
        .map_err(|source| DatabaseError::Query {
            table: "relates_to",
            operation: "outgoing_edges",
            source,
        })?;

    response.take(0).map_err(|source| DatabaseError::Query {
        table: "relates_to",
        operation: "outgoing_edges/take",
        source,
    })
}

#[tracing::instrument(skip_all, level = "debug", fields(note_id = %note_id))]
pub async fn incoming_edges(
    db: &Surreal<Db>,
    note_id: &Thing,
) -> Result<Vec<EdgeRecord>, DatabaseError> {
    let mut response = db
        .query("SELECT * FROM relates_to WHERE out = $note_id")
        .bind(("note_id", note_id.clone()))
        .await
        .map_err(|source| DatabaseError::Query {
            table: "relates_to",
            operation: "incoming_edges",
            source,
        })?;

    response.take(0).map_err(|source| DatabaseError::Query {
        table: "relates_to",
        operation: "incoming_edges/take",
        source,
    })
}

#[tracing::instrument(skip_all, level = "debug", fields(note_id = %note_id))]
pub async fn incoming_cited(
    db: &Surreal<Db>,
    note_id: &Thing,
) -> Result<Vec<Thing>, DatabaseError> {
    let mut response = db
        .query("SELECT `in` FROM cited WHERE out = $note_id")
        .bind(("note_id", note_id.clone()))
        .await
        .map_err(|source| DatabaseError::Query {
            table: "cited",
            operation: "incoming_cited",
            source,
        })?;

    let rows: Vec<InRow> = response.take(0).map_err(|source| DatabaseError::Query {
        table: "cited",
        operation: "incoming_cited/take",
        source,
    })?;

    Ok(rows.into_iter().map(|row| row.in_node).collect())
}

#[tracing::instrument(skip_all, level = "debug", fields(note_id = %note_id))]
pub async fn delete_outgoing_edges(db: &Surreal<Db>, note_id: &Thing) -> Result<(), DatabaseError> {
    db.query("DELETE relates_to WHERE `in` = $note_id")
        .bind(("note_id", note_id.clone()))
        .await
        .map_err(|source| DatabaseError::Query {
            table: "relates_to",
            operation: "delete_outgoing",
            source,
        })?;
    Ok(())
}

#[tracing::instrument(skip_all, level = "debug")]
pub async fn orphan_notes(db: &Surreal<Db>) -> Result<Vec<NoteRecord>, DatabaseError> {
    let mut response = db
        .query(
            "SELECT * FROM note WHERE \
             count(->relates_to) = 0 AND \
             count(<-relates_to) = 0",
        )
        .await
        .map_err(|source| DatabaseError::Query {
            table: "note",
            operation: "orphan_notes",
            source,
        })?;

    response.take(0).map_err(|source| DatabaseError::Query {
        table: "note",
        operation: "orphan_notes/take",
        source,
    })
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
    let mut response = db
        .query("SELECT count() AS count FROM note WHERE body = '' AND trust = 1 GROUP ALL")
        .await
        .map_err(|source| DatabaseError::Query {
            table: "note",
            operation: "count_stubs",
            source,
        })?;

    let rows: Vec<CountRow> = response.take(0).map_err(|source| DatabaseError::Query {
        table: "note",
        operation: "count_stubs/take",
        source,
    })?;

    Ok(rows.first().map_or(0, |r| r.count))
}

#[derive(Debug, Deserialize)]
struct TagsRow {
    tags: Vec<String>,
}

#[tracing::instrument(skip_all, level = "debug")]
pub async fn list_tags_with_counts(db: &Surreal<Db>) -> Result<Vec<(String, i64)>, DatabaseError> {
    let mut response = db
        .query("SELECT tags FROM note WHERE array::len(tags) > 0")
        .await
        .map_err(|source| DatabaseError::Query {
            table: "note",
            operation: "list_tags",
            source,
        })?;

    let rows: Vec<TagsRow> = response.take(0).map_err(|source| DatabaseError::Query {
        table: "note",
        operation: "list_tags/take",
        source,
    })?;

    let mut counts: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    for row in rows {
        for tag in row.tags {
            *counts.entry(tag).or_insert(0) += 1;
        }
    }

    let mut result: Vec<(String, i64)> = counts.into_iter().collect();
    result.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    Ok(result)
}

// --- Reference updates ---

#[tracing::instrument(skip_all, level = "debug", fields(ref_id = %ref_id))]
pub async fn update_reference_path(
    db: &Surreal<Db>,
    ref_id: &Thing,
    new_path: &str,
    new_topic: &str,
) -> Result<(), DatabaseError> {
    db.query("UPDATE $ref_id SET path = $path, topic = $topic")
        .bind(("ref_id", ref_id.clone()))
        .bind(("path", new_path.to_string()))
        .bind(("topic", new_topic.to_string()))
        .await
        .map_err(|source| DatabaseError::Query {
            table: "reference",
            operation: "update_path",
            source,
        })?;
    Ok(())
}

#[tracing::instrument(skip_all, level = "debug", fields(path = %path))]
pub async fn find_reference_by_path(
    db: &Surreal<Db>,
    path: &str,
) -> Result<Option<ReferenceRecord>, DatabaseError> {
    let mut response = db
        .query("SELECT * FROM reference WHERE path = $path LIMIT 1")
        .bind(("path", path.to_string()))
        .await
        .map_err(|source| DatabaseError::Query {
            table: "reference",
            operation: "find_by_path",
            source,
        })?;

    let rows: Vec<ReferenceRecord> = response.take(0).map_err(|source| DatabaseError::Query {
        table: "reference",
        operation: "find_by_path/take",
        source,
    })?;

    Ok(rows.into_iter().next())
}

// --- Helpers ---

async fn count_table(db: &Surreal<Db>, table: &'static str) -> Result<i64, DatabaseError> {
    let query = format!("SELECT count() AS count FROM {table} GROUP ALL");
    let mut response = db
        .query(&query)
        .await
        .map_err(|source| DatabaseError::Query {
            table,
            operation: "count",
            source,
        })?;

    let rows: Vec<CountRow> = response.take(0).map_err(|source| DatabaseError::Query {
        table,
        operation: "count/take",
        source,
    })?;

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
