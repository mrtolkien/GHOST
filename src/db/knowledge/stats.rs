use serde::Deserialize;
use surrealdb::Surreal;
use surrealdb::engine::local::Db;

use crate::db::error::DatabaseError;
use crate::db::query::{CountRow, query_exec, take_many};

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

async fn count_table(db: &Surreal<Db>, table: &'static str) -> Result<i64, DatabaseError> {
    let query = format!("SELECT count() AS count FROM {table} GROUP ALL");
    let mut resp = query_exec(db.query(&query), table, "count").await?;
    let rows: Vec<CountRow> = take_many(&mut resp, 0, table, "count")?;
    Ok(rows.first().map_or(0, |r| r.count))
}
