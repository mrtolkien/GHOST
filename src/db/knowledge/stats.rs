use sqlx::SqlitePool;

use crate::db::error::DatabaseError;

#[tracing::instrument(skip_all, level = "debug")]
pub async fn count_notes(db: &SqlitePool) -> Result<i64, DatabaseError> {
    count_table(db, "note").await
}

#[tracing::instrument(skip_all, level = "debug")]
pub async fn count_references(db: &SqlitePool) -> Result<i64, DatabaseError> {
    count_table(db, "reference").await
}

#[tracing::instrument(skip_all, level = "debug")]
pub async fn count_diary(db: &SqlitePool) -> Result<i64, DatabaseError> {
    count_table(db, "diary").await
}

#[tracing::instrument(skip_all, level = "debug")]
pub async fn count_edges(db: &SqlitePool) -> Result<i64, DatabaseError> {
    count_table(db, "relates_to").await
}

#[tracing::instrument(skip_all, level = "debug")]
pub async fn count_stubs(db: &SqlitePool) -> Result<i64, DatabaseError> {
    let (count,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM note WHERE body = '' AND trust = 1")
            .fetch_one(db)
            .await
            .map_err(|source| DatabaseError::Query {
                table: "note",
                operation: "count_stubs",
                source,
            })?;
    Ok(count)
}

#[tracing::instrument(skip_all, level = "debug")]
pub async fn list_tags_with_counts(db: &SqlitePool) -> Result<Vec<(String, i64)>, DatabaseError> {
    #[derive(sqlx::FromRow)]
    struct TagCountRow {
        tag: String,
        count: i64,
    }

    let rows = sqlx::query_as::<_, TagCountRow>(
        "SELECT j.value AS tag, COUNT(*) AS count \
         FROM note, json_each(note.tags) AS j \
         GROUP BY j.value \
         ORDER BY count DESC",
    )
    .fetch_all(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "note",
        operation: "list_tags",
        source,
    })?;

    Ok(rows.into_iter().map(|r| (r.tag, r.count)).collect())
}

async fn count_table(db: &SqlitePool, table: &'static str) -> Result<i64, DatabaseError> {
    let query = format!("SELECT COUNT(*) FROM {table}");
    let (count,): (i64,) = sqlx::query_as(&query)
        .fetch_one(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table,
            operation: "count",
            source,
        })?;
    Ok(count)
}
