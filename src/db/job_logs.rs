use serde::Deserialize;
use surrealdb::Surreal;
use surrealdb::engine::local::Db;
use surrealdb::sql::{Datetime, Thing};

use crate::db::error::DatabaseError;

#[derive(Debug, Deserialize)]
struct IdRow {
    id: Thing,
}

#[derive(Debug, Clone, Deserialize)]
pub struct JobLogRecord {
    pub id: Thing,
    pub job_name: String,
    pub job_kind: String,
    pub started_at: Datetime,
    pub finished_at: Option<Datetime>,
    pub status: String,
    pub transcript: Option<String>,
}

#[tracing::instrument(skip_all, level = "debug", fields(job_name = job_name, job_kind = job_kind))]
pub async fn create_running_job_log(
    db: &Surreal<Db>,
    job_name: &str,
    job_kind: &str,
    session_id: Option<&Thing>,
) -> Result<Thing, DatabaseError> {
    let mut response = db
        .query(
            "CREATE job_log SET \
                job_name = $job_name, \
                job_kind = $job_kind, \
                session = $session_id, \
                started_at = time::now(), \
                finished_at = NONE, \
                status = 'running', \
                transcript = NONE, \
                handoff_note = NONE, \
                todo_list = NONE \
             RETURN id",
        )
        .bind(("job_name", job_name.to_string()))
        .bind(("job_kind", job_kind.to_string()))
        .bind(("session_id", session_id.cloned()))
        .await
        .map_err(|source| DatabaseError::Query {
            table: "job_log",
            operation: "create_running",
            source,
        })?;

    let rows: Vec<IdRow> = response.take(0).map_err(|source| DatabaseError::Query {
        table: "job_log",
        operation: "create_running/take",
        source,
    })?;

    rows.into_iter()
        .next()
        .map(|row| row.id)
        .ok_or(DatabaseError::MissingRow {
            table: "job_log",
            operation: "create_running",
        })
}

#[tracing::instrument(skip_all, level = "debug", fields(job_log_id = %job_log_id, status = status))]
pub async fn finish_job_log(
    db: &Surreal<Db>,
    job_log_id: &Thing,
    status: &str,
    transcript: &str,
) -> Result<(), DatabaseError> {
    db.query(
        "UPDATE $job_log_id SET \
            status = $status, \
            transcript = $transcript, \
            finished_at = time::now()",
    )
    .bind(("job_log_id", job_log_id.clone()))
    .bind(("status", status.to_string()))
    .bind(("transcript", transcript.to_string()))
    .await
    .map_err(|source| DatabaseError::Query {
        table: "job_log",
        operation: "finish",
        source,
    })?;

    Ok(())
}

#[tracing::instrument(skip_all, level = "debug", fields(name = name, limit = limit))]
pub async fn list_job_logs(
    db: &Surreal<Db>,
    name: Option<&str>,
    limit: usize,
) -> Result<Vec<JobLogRecord>, DatabaseError> {
    let query = match name {
        Some(_) => {
            "SELECT * FROM job_log WHERE job_name = $name \
             ORDER BY started_at DESC LIMIT $limit"
        }
        None => {
            "SELECT * FROM job_log \
             ORDER BY started_at DESC LIMIT $limit"
        }
    };

    let mut response = db
        .query(query)
        .bind(("name", name.map(|n| n.to_string())))
        .bind(("limit", limit))
        .await
        .map_err(|source| DatabaseError::Query {
            table: "job_log",
            operation: "list",
            source,
        })?;

    let rows: Vec<JobLogRecord> = response.take(0).map_err(|source| DatabaseError::Query {
        table: "job_log",
        operation: "list/take",
        source,
    })?;

    Ok(rows)
}
