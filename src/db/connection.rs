use std::path::Path;
use std::sync::Once;

use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use tracing::info;

use super::error::DatabaseError;

pub type GhostDb = SqlitePool;

static SQLITE_VEC_INIT: Once = Once::new();

#[tracing::instrument(skip_all, fields(db_path = %workspace.join("ghost.db").display()))]
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
        .busy_timeout(std::time::Duration::from_secs(5))
        .pragma("cache_size", "-65536"); // 64 MB

    let pool = SqlitePoolOptions::new()
        .connect_with(opts)
        .await
        .map_err(|source| DatabaseError::Connect {
            path: db_path.clone(),
            source,
        })?;

    let start = std::time::Instant::now();

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .map_err(|source| DatabaseError::Migrate { source })?;

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
