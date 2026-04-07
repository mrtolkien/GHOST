use std::path::Path;

use sqlx::Connection;
use sqlx::sqlite::{SqliteConnectOptions, SqliteConnection, SqliteJournalMode};

use crate::db::DatabaseError;

pub const DB_ONLY_TABLES: &[&str] = &[
    "session",
    "message",
    "interface_session",
    "agent_run",
    "usage_log",
    "agent_state",
    "coding_sessions",
    "message_source",
];

pub const FILE_BACKED_TABLES: &[&str] = &[
    "topic",
    "note",
    "reference",
    "import_batch",
    "diary",
    "script",
    "code_file",
    "relates_to",
    "cited",
    "embedding",
    "vec_embedding",
    "note_fts",
    "reference_fts",
    "diary_fts",
    "script_fts",
    "code_file_fts",
];

const SOURCE_SCHEMA: &str = "source_db";

pub async fn copy_db_only_tables(
    candidate_db_path: &Path,
    live_db_path: &Path,
) -> Result<(), DatabaseError> {
    let mut conn = connect_sqlite(candidate_db_path).await?;
    attach_source_db(&mut conn, live_db_path).await?;

    for table in DB_ONLY_TABLES {
        copy_table(&mut conn, table).await?;
    }
    copy_message_sources(&mut conn).await?;

    detach_source_db(&mut conn).await?;
    conn.close()
        .await
        .map_err(|source| DatabaseError::Connect {
            path: candidate_db_path.to_path_buf(),
            source,
        })?;
    Ok(())
}

pub async fn count_rows(
    candidate_db_path: &Path,
    live_db_path: &Path,
    table: &'static str,
) -> Result<(u64, u64), DatabaseError> {
    let mut conn = connect_sqlite(candidate_db_path).await?;
    attach_source_db(&mut conn, live_db_path).await?;
    let source_rows = count_table_rows(&mut conn, &format!("{SOURCE_SCHEMA}.{table}")).await?;
    let candidate_rows = count_table_rows(&mut conn, table).await?;
    detach_source_db(&mut conn).await?;
    conn.close()
        .await
        .map_err(|source| DatabaseError::Connect {
            path: candidate_db_path.to_path_buf(),
            source,
        })?;
    Ok((source_rows, candidate_rows))
}

async fn copy_table(conn: &mut SqliteConnection, table: &'static str) -> Result<(), DatabaseError> {
    if table == "message_source" {
        return Ok(());
    }

    sqlx::query(&format!(
        "INSERT INTO {table} SELECT * FROM {SOURCE_SCHEMA}.{table}"
    ))
    .execute(&mut *conn)
    .await
    .map_err(|source| DatabaseError::Query {
        table,
        operation: "repair_copy",
        source,
    })?;
    Ok(())
}

async fn copy_message_sources(conn: &mut SqliteConnection) -> Result<(), DatabaseError> {
    ensure_message_source_reference_mapping(conn).await?;
    sqlx::query(&format!(
        "INSERT INTO message_source (id, message_id, reference_id, url, title, created_at)
         SELECT ms.id,
                ms.message_id,
                (
                    SELECT r.id
                    FROM reference r
                    WHERE r.source_url = ms.url
                    ORDER BY r.created_at ASC
                    LIMIT 1
                ) AS reference_id,
                ms.url,
                ms.title,
                ms.created_at
         FROM {SOURCE_SCHEMA}.message_source ms"
    ))
    .execute(&mut *conn)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "message_source",
        operation: "repair_copy",
        source,
    })?;
    Ok(())
}

async fn ensure_message_source_reference_mapping(
    conn: &mut SqliteConnection,
) -> Result<(), DatabaseError> {
    let ambiguous_or_missing = sqlx::query_scalar::<_, i64>(&format!(
        "SELECT COUNT(*)
         FROM {SOURCE_SCHEMA}.message_source ms
         LEFT JOIN (
             SELECT source_url, COUNT(*) AS match_count
             FROM reference
             WHERE source_url IS NOT NULL
             GROUP BY source_url
         ) candidate_refs
         ON candidate_refs.source_url = ms.url
         WHERE ms.reference_id IS NOT NULL
           AND COALESCE(candidate_refs.match_count, 0) != 1"
    ))
    .fetch_one(&mut *conn)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "message_source",
        operation: "verify_reference_mapping",
        source,
    })?;

    if ambiguous_or_missing > 0 {
        return Err(DatabaseError::Other(format!(
            "cannot safely repair message_source: {ambiguous_or_missing} rows have missing or ambiguous rebuilt reference mappings"
        )));
    }
    Ok(())
}

async fn count_table_rows(conn: &mut SqliteConnection, table: &str) -> Result<u64, DatabaseError> {
    let query = format!("SELECT COUNT(*) AS count FROM {table}");
    let count = sqlx::query_scalar::<_, i64>(&query)
        .fetch_one(&mut *conn)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "repair_count",
            operation: "count_rows",
            source,
        })?;
    Ok(u64::try_from(count.max(0)).unwrap_or_default())
}

async fn connect_sqlite(path: &Path) -> Result<SqliteConnection, DatabaseError> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .journal_mode(SqliteJournalMode::Wal)
        .foreign_keys(true);
    SqliteConnection::connect_with(&options)
        .await
        .map_err(|source| DatabaseError::Connect {
            path: path.to_path_buf(),
            source,
        })
}

async fn attach_source_db(
    conn: &mut SqliteConnection,
    live_db_path: &Path,
) -> Result<(), DatabaseError> {
    let live_db_path = live_db_path.to_string_lossy().into_owned();
    sqlx::query(&format!("ATTACH DATABASE ? AS {SOURCE_SCHEMA}"))
        .bind(live_db_path)
        .execute(&mut *conn)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "repair_attach",
            operation: "attach_source_db",
            source,
        })?;
    Ok(())
}

async fn detach_source_db(conn: &mut SqliteConnection) -> Result<(), DatabaseError> {
    sqlx::query(&format!("DETACH DATABASE {SOURCE_SCHEMA}"))
        .execute(&mut *conn)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "repair_attach",
            operation: "detach_source_db",
            source,
        })?;
    Ok(())
}
