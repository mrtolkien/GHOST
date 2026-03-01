use sqlx::{Row, SqlitePool};

use super::DatabaseError;

/// Get an agent state value by slug and key.
pub async fn get_state(
    db: &SqlitePool,
    agent_slug: &str,
    key: &str,
) -> Result<Option<String>, DatabaseError> {
    let row = sqlx::query("SELECT value FROM agent_state WHERE agent_slug = ? AND key = ?")
        .bind(agent_slug)
        .bind(key)
        .fetch_optional(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "agent_state",
            operation: "get",
            source,
        })?;

    Ok(row.map(|r| r.get("value")))
}

/// Set an agent state value (upsert).
pub async fn set_state(
    db: &SqlitePool,
    agent_slug: &str,
    key: &str,
    value: &str,
) -> Result<(), DatabaseError> {
    let now = super::now();
    sqlx::query(
        "INSERT INTO agent_state (agent_slug, key, value, updated_at)
         VALUES (?, ?, ?, ?)
         ON CONFLICT (agent_slug, key) DO UPDATE SET value = ?, updated_at = ?",
    )
    .bind(agent_slug)
    .bind(key)
    .bind(value)
    .bind(&now)
    .bind(value)
    .bind(&now)
    .execute(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "agent_state",
        operation: "set",
        source,
    })?;

    Ok(())
}

/// Delete an agent state value.
pub async fn delete_state(
    db: &SqlitePool,
    agent_slug: &str,
    key: &str,
) -> Result<(), DatabaseError> {
    sqlx::query("DELETE FROM agent_state WHERE agent_slug = ? AND key = ?")
        .bind(agent_slug)
        .bind(key)
        .execute(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "agent_state",
            operation: "delete",
            source,
        })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_db() -> SqlitePool {
        let db = SqlitePool::connect(":memory:").await.unwrap();
        sqlx::migrate!().run(&db).await.unwrap();
        db
    }

    #[tokio::test]
    async fn round_trip_get_set_delete() {
        let db = test_db().await;

        // Initially empty
        let val = get_state(&db, "deep-research", "last_run").await.unwrap();
        assert!(val.is_none());

        // Set
        set_state(&db, "deep-research", "last_run", "2026-03-01")
            .await
            .unwrap();
        let val = get_state(&db, "deep-research", "last_run").await.unwrap();
        assert_eq!(val.as_deref(), Some("2026-03-01"));

        // Update (upsert)
        set_state(&db, "deep-research", "last_run", "2026-03-02")
            .await
            .unwrap();
        let val = get_state(&db, "deep-research", "last_run").await.unwrap();
        assert_eq!(val.as_deref(), Some("2026-03-02"));

        // Delete
        delete_state(&db, "deep-research", "last_run")
            .await
            .unwrap();
        let val = get_state(&db, "deep-research", "last_run").await.unwrap();
        assert!(val.is_none());
    }

    #[tokio::test]
    async fn different_slugs_are_isolated() {
        let db = test_db().await;

        set_state(&db, "agent-a", "key1", "val-a").await.unwrap();
        set_state(&db, "agent-b", "key1", "val-b").await.unwrap();

        let a = get_state(&db, "agent-a", "key1").await.unwrap();
        let b = get_state(&db, "agent-b", "key1").await.unwrap();
        assert_eq!(a.as_deref(), Some("val-a"));
        assert_eq!(b.as_deref(), Some("val-b"));
    }
}
