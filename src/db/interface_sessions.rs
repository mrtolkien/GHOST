//! TEMPORARY SCAFFOLDING
//! This is the minimal interface-session mapping layer needed by spec 06 reboot flow.
//! It is expected to be revisited/reworked in full Discord interface work (spec 09).

use serde::Deserialize;
use sqlx::SqlitePool;

use crate::db::error::DatabaseError;
use crate::db::{new_id, now};

#[tracing::instrument(skip_all, level = "debug", fields(interface = interface))]
pub async fn get_active_session_for_interface(
    db: &SqlitePool,
    interface: &str,
) -> Result<Option<String>, DatabaseError> {
    #[derive(sqlx::FromRow)]
    struct SessionRow {
        session_id: String,
    }

    let row = sqlx::query_as::<_, SessionRow>(
        "SELECT session_id FROM interface_session WHERE interface = ? LIMIT 1",
    )
    .bind(interface)
    .fetch_optional(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "interface_session",
        operation: "get_active",
        source,
    })?;

    Ok(row.map(|r| r.session_id))
}

#[tracing::instrument(skip_all, level = "debug", fields(interface = interface, session_id = %session_id))]
pub async fn set_active_session_for_interface(
    db: &SqlitePool,
    interface: &str,
    session_id: &str,
) -> Result<(), DatabaseError> {
    let id = new_id();
    sqlx::query(
        "INSERT INTO interface_session (id, interface, session_id, created_at) \
         VALUES (?, ?, ?, ?) \
         ON CONFLICT(interface) DO UPDATE SET session_id = excluded.session_id",
    )
    .bind(&id)
    .bind(interface)
    .bind(session_id)
    .bind(now())
    .execute(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "interface_session",
        operation: "set_active",
        source,
    })?;
    Ok(())
}

#[derive(Debug, Deserialize, sqlx::FromRow)]
pub struct InterfaceSessionRecord {
    pub interface: String,
    pub session_id: String,
}

#[tracing::instrument(skip_all, level = "debug")]
pub async fn list_all_interface_sessions(
    db: &SqlitePool,
) -> Result<Vec<InterfaceSessionRecord>, DatabaseError> {
    sqlx::query_as::<_, InterfaceSessionRecord>(
        "SELECT interface, session_id FROM interface_session",
    )
    .fetch_all(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "interface_session",
        operation: "list_all",
        source,
    })
}

#[tracing::instrument(skip_all, level = "debug", fields(old_session_id = %old_session_id, new_session_id = %new_session_id))]
pub async fn replace_session_everywhere(
    db: &SqlitePool,
    old_session_id: &str,
    new_session_id: &str,
) -> Result<(), DatabaseError> {
    sqlx::query("UPDATE interface_session SET session_id = ? WHERE session_id = ?")
        .bind(new_session_id)
        .bind(old_session_id)
        .execute(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "interface_session",
            operation: "replace_session_everywhere",
            source,
        })?;
    Ok(())
}
