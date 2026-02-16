use serde::{Deserialize, Serialize};
use surrealdb::Surreal;
use surrealdb::engine::local::Db;
use surrealdb::sql::{Datetime, Thing};

use crate::db::error::DatabaseError;

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

#[derive(Debug, Deserialize)]
struct IdRow {
    id: Thing,
}

#[derive(Debug, Deserialize)]
struct OutRow {
    out: Thing,
}

#[tracing::instrument(skip_all, fields(title = %title))]
pub async fn create_note(
    db: &Surreal<Db>,
    title: &str,
    body: &str,
) -> Result<Thing, DatabaseError> {
    let title = title.to_owned();
    let body = body.to_owned();

    let mut response = db
        .query(
            "CREATE note SET \
                title = $title, \
                body = $body, \
                archetype = NONE, \
                tags = [], \
                trust = 5, \
                created_at = time::now(), \
                updated_at = time::now() \
             RETURN id",
        )
        .bind(("title", title))
        .bind(("body", body))
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

#[tracing::instrument(skip_all, fields(topic = %topic, path = %path))]
pub async fn create_reference(
    db: &Surreal<Db>,
    topic: &str,
    path: &str,
    content: &str,
    source_url: Option<&str>,
) -> Result<Thing, DatabaseError> {
    // TEMPORARY SCAFFOLDING:
    // Added for spec 06 citation linkage. The full reference lifecycle and write flows
    // belong to spec 13 and may replace this helper entirely.
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

#[tracing::instrument(skip_all, fields(note_id = %note_id))]
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

#[tracing::instrument(skip_all, fields(from = %from, to = %to, label = %label))]
pub async fn create_edge(
    db: &Surreal<Db>,
    from: &Thing,
    to: &Thing,
    label: &str,
) -> Result<Thing, DatabaseError> {
    let label = label.to_owned();

    let mut response = db
        .query(
            "RELATE $from->relates_to->$to SET label = $label, created_at = time::now() RETURN id",
        )
        .bind(("from", from.clone()))
        .bind(("to", to.clone()))
        .bind(("label", label))
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

#[tracing::instrument(skip_all, fields(from = %from))]
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
