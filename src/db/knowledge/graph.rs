use serde::Deserialize;
use surrealdb::Surreal;
use surrealdb::engine::local::Db;
use surrealdb::types::{RecordId, SurrealValue};

use crate::db::error::DatabaseError;
use crate::db::query::{IdRow, query_exec, take_many, take_one};

use super::records::{EdgeRecord, NoteRecord};

#[derive(Debug, Deserialize, SurrealValue)]
#[surreal(crate = "surrealdb::types")]
struct OutRow {
    out: RecordId,
}

#[derive(Debug, Deserialize, SurrealValue)]
#[surreal(crate = "surrealdb::types")]
struct InRow {
    #[serde(rename = "in")]
    #[surreal(rename = "in")]
    in_node: RecordId,
}

#[tracing::instrument(skip_all, level = "debug", fields(from = ?from, to = ?to, label = %label))]
pub async fn create_edge(
    db: &Surreal<Db>,
    from: &RecordId,
    to: &RecordId,
    label: &str,
) -> Result<RecordId, DatabaseError> {
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

#[tracing::instrument(skip_all, level = "debug", fields(from = ?from))]
pub async fn related_note_ids(
    db: &Surreal<Db>,
    from: &RecordId,
) -> Result<Vec<RecordId>, DatabaseError> {
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

#[tracing::instrument(skip_all, level = "debug", fields(note_id = ?note_id))]
pub async fn outgoing_edges(
    db: &Surreal<Db>,
    note_id: &RecordId,
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

#[tracing::instrument(skip_all, level = "debug", fields(note_id = ?note_id))]
pub async fn incoming_edges(
    db: &Surreal<Db>,
    note_id: &RecordId,
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

#[tracing::instrument(skip_all, level = "debug", fields(note_id = ?note_id))]
pub async fn incoming_cited(
    db: &Surreal<Db>,
    note_id: &RecordId,
) -> Result<Vec<RecordId>, DatabaseError> {
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

#[tracing::instrument(skip_all, level = "debug", fields(from = ?from, to = ?to))]
pub async fn create_cited_edge(
    db: &Surreal<Db>,
    from: &RecordId,
    to: &RecordId,
) -> Result<RecordId, DatabaseError> {
    let mut resp = query_exec(
        db.query(
            "RELATE $from->cited->$to \
             SET created_at = time::now() \
             RETURN id",
        )
        .bind(("from", from.clone()))
        .bind(("to", to.clone())),
        "cited",
        "create_cited_edge",
    )
    .await?;

    let row: IdRow = take_one(&mut resp, 0, "cited", "create_cited_edge")?;
    Ok(row.id)
}

#[tracing::instrument(skip_all, level = "debug", fields(note_id = ?note_id))]
pub async fn delete_outgoing_edges(
    db: &Surreal<Db>,
    note_id: &RecordId,
) -> Result<(), DatabaseError> {
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
