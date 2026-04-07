use std::path::Path;
use std::sync::Once;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::{Connection, SqliteConnection, SqlitePool};
use tracing::info;

use super::error::DatabaseError;

/// SQLite busy timeout for lock contention.
const BUSY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// SQLite page cache size (negative = KiB). -65536 = 64 MB.
const CACHE_SIZE_KIB: &str = "-65536";

pub type GhostDb = SqlitePool;

static SQLITE_VEC_INIT: Once = Once::new();

#[tracing::instrument(skip_all, err, fields(db_path = %workspace.join("ghost.db").display()))]
pub async fn connect(workspace: &Path, embedding_dim: usize) -> Result<GhostDb, DatabaseError> {
    // Register sqlite-vec extension once per process.
    // SAFETY: must be called before any SQLite connections are opened.
    SQLITE_VEC_INIT.call_once(|| unsafe {
        libsqlite3_sys::sqlite3_auto_extension(Some(std::mem::transmute::<
            *const (),
            unsafe extern "C" fn(
                *mut libsqlite3_sys::sqlite3,
                *mut *mut std::ffi::c_char,
                *const libsqlite3_sys::sqlite3_api_routines,
            ) -> std::ffi::c_int,
        >(
            sqlite_vec::sqlite3_vec_init as *const ()
        )));
    });

    let db_path = workspace.join("ghost.db");

    let opts = SqliteConnectOptions::new()
        .filename(&db_path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .foreign_keys(true)
        .busy_timeout(BUSY_TIMEOUT)
        .pragma("cache_size", CACHE_SIZE_KIB);

    // Run migrations on a dedicated single connection BEFORE opening the pool.
    // SQLite DDL (CREATE/DROP TABLE) requires an exclusive lock. Running
    // migrations through a pool risks SQLITE_LOCKED (code 6) because the
    // pool's other connections hold reader locks that block the exclusive lock.
    let start = std::time::Instant::now();

    let mut migrate_conn = SqliteConnection::connect_with(&opts)
        .await
        .map_err(|source| DatabaseError::Connect {
            path: db_path.clone(),
            source,
        })?;

    sqlx::migrate!("./migrations")
        .run(&mut migrate_conn)
        .await
        .map_err(|source| DatabaseError::Migrate { source })?;

    migrate_conn.close().await.ok();

    let pool = SqlitePoolOptions::new()
        // sqlx's SQLite ping() is a no-op (only checks the worker thread
        // channel, not the actual SQLite handle). A connection that hits a
        // transient I/O error stays in the pool and poisons every subsequent
        // query. This callback runs a real query to detect broken connections
        // so the pool can replace them. Near-zero cost for SQLite (in-process).
        .test_before_acquire(false)
        .before_acquire(|conn, _meta| {
            Box::pin(async move {
                sqlx::query("SELECT 1").execute(&mut *conn).await?;
                Ok(true)
            })
        })
        .connect_with(opts)
        .await
        .map_err(|source| DatabaseError::Connect {
            path: db_path.clone(),
            source,
        })?;

    // Create vec0 virtual table with dynamic embedding dimension.
    // This lives outside the migration because the dimension comes from config.
    sqlx::query(&format!(
        "CREATE VIRTUAL TABLE IF NOT EXISTS vec_embedding \
         USING vec0(embedding float[{embedding_dim}] distance_metric=cosine)"
    ))
    .execute(&pool)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "vec_embedding",
        operation: "create_virtual_table",
        source,
    })?;

    info!(
        elapsed_ms = start.elapsed().as_millis() as u64,
        "schema applied"
    );

    Ok(pool)
}
