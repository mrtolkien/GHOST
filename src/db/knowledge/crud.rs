use sqlx::SqlitePool;

use crate::db::error::DatabaseError;
use crate::db::{new_id, now};

use super::records::{
    CodeFileRecord, DiaryRecord, NoteRecord, RecentItem, ReferenceRecord, ScriptRecord,
};

// --- Create / Update ---

#[tracing::instrument(skip_all, level = "debug", fields(title = %title))]
pub async fn create_note(
    db: &SqlitePool,
    title: &str,
    body: &str,
) -> Result<String, DatabaseError> {
    create_note_full(db, title, body, &[], &[], 5, None, None, None, None).await
}

#[allow(clippy::too_many_arguments)]
#[tracing::instrument(skip_all, level = "debug", fields(title = %title))]
pub async fn create_note_full(
    db: &SqlitePool,
    title: &str,
    body: &str,
    tags: &[String],
    sources: &[String],
    trust: i64,
    archetype: Option<&str>,
    topic_id: Option<&str>,
    path: Option<&str>,
    file_hash: Option<&str>,
) -> Result<String, DatabaseError> {
    let id = new_id();
    let ts = now();
    let tags_json = serde_json::to_string(tags).unwrap_or_default();
    let sources_json = serde_json::to_string(sources).unwrap_or_default();

    sqlx::query(
        "INSERT INTO note \
         (id, title, body, tags, sources, trust, archetype, topic_id, path, file_hash, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(title)
    .bind(body)
    .bind(&tags_json)
    .bind(&sources_json)
    .bind(trust)
    .bind(archetype)
    .bind(topic_id)
    .bind(path)
    .bind(file_hash)
    .bind(&ts)
    .bind(&ts)
    .execute(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "note",
        operation: "create",
        source,
    })?;

    Ok(id)
}

#[allow(clippy::too_many_arguments)]
#[tracing::instrument(skip_all, level = "debug", fields(note_id = %note_id))]
pub async fn update_note(
    db: &SqlitePool,
    note_id: &str,
    body: &str,
    tags: &[String],
    sources: &[String],
    trust: i64,
    archetype: Option<&str>,
    topic_id: Option<&str>,
    path: Option<&str>,
    file_hash: Option<&str>,
) -> Result<(), DatabaseError> {
    let tags_json = serde_json::to_string(tags).unwrap_or_default();
    let sources_json = serde_json::to_string(sources).unwrap_or_default();

    sqlx::query(
        "UPDATE note SET body = ?, tags = ?, sources = ?, \
         trust = ?, archetype = ?, topic_id = COALESCE(?, topic_id), path = ?, file_hash = ?, \
         updated_at = ? WHERE id = ?",
    )
    .bind(body)
    .bind(&tags_json)
    .bind(&sources_json)
    .bind(trust)
    .bind(archetype)
    .bind(topic_id)
    .bind(path)
    .bind(file_hash)
    .bind(now())
    .bind(note_id)
    .execute(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "note",
        operation: "update",
        source,
    })?;
    Ok(())
}

#[tracing::instrument(skip_all, level = "debug", fields(date = %date))]
pub async fn create_diary(
    db: &SqlitePool,
    date: &str,
    body: &str,
    file_hash: Option<&str>,
) -> Result<String, DatabaseError> {
    let id = new_id();

    sqlx::query("INSERT INTO diary (id, date, body, file_hash, updated_at) VALUES (?, ?, ?, ?, ?)")
        .bind(&id)
        .bind(date)
        .bind(body)
        .bind(file_hash)
        .bind(now())
        .execute(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "diary",
            operation: "create",
            source,
        })?;

    Ok(id)
}

#[tracing::instrument(skip_all, level = "debug", fields(diary_id = %diary_id))]
pub async fn update_diary(
    db: &SqlitePool,
    diary_id: &str,
    body: &str,
    file_hash: Option<&str>,
) -> Result<(), DatabaseError> {
    sqlx::query("UPDATE diary SET body = ?, file_hash = ?, updated_at = ? WHERE id = ?")
        .bind(body)
        .bind(file_hash)
        .bind(now())
        .bind(diary_id)
        .execute(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "diary",
            operation: "update",
            source,
        })?;
    Ok(())
}

#[tracing::instrument(skip_all, level = "debug", fields(date = %date))]
pub async fn append_diary(db: &SqlitePool, date: &str, line: &str) -> Result<(), DatabaseError> {
    sqlx::query("UPDATE diary SET body = body || char(10) || ?, updated_at = ? WHERE date = ?")
        .bind(line)
        .bind(now())
        .bind(date)
        .execute(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "diary",
            operation: "append",
            source,
        })?;
    Ok(())
}

#[tracing::instrument(skip_all, level = "debug", fields(topic_id = %topic_id, path = %path))]
pub async fn create_reference(
    db: &SqlitePool,
    topic_id: &str,
    path: &str,
    content: &str,
    source_url: Option<&str>,
    import_batch_id: Option<&str>,
    file_hash: Option<&str>,
) -> Result<String, DatabaseError> {
    let id = new_id();
    let ts = now();

    // Upsert: if a reference with the same (topic_id, path) already exists,
    // update its content in place. This handles re-imports gracefully — e.g.
    // when a long-running PDF import is retried after a timeout.
    let row = sqlx::query_as::<_, (String,)>(
        "INSERT INTO reference (id, topic_id, path, content, source_url, import_batch_id, file_hash, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?) \
         ON CONFLICT(topic_id, path) DO UPDATE SET \
           content = excluded.content, \
           source_url = excluded.source_url, \
           import_batch_id = excluded.import_batch_id, \
           file_hash = excluded.file_hash \
         RETURNING id",
    )
    .bind(&id)
    .bind(topic_id)
    .bind(path)
    .bind(content)
    .bind(source_url)
    .bind(import_batch_id)
    .bind(file_hash)
    .bind(&ts)
    .fetch_one(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "reference",
        operation: "create",
        source,
    })?;

    Ok(row.0)
}

// --- Read ---

#[tracing::instrument(skip_all, level = "debug", fields(note_id = %note_id))]
pub async fn get_note(db: &SqlitePool, note_id: &str) -> Result<NoteRecord, DatabaseError> {
    sqlx::query_as::<_, NoteRecord>("SELECT * FROM note WHERE id = ?")
        .bind(note_id)
        .fetch_optional(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "note",
            operation: "get",
            source,
        })?
        .ok_or(DatabaseError::MissingRow {
            table: "note",
            operation: "get",
        })
}

#[tracing::instrument(skip_all, level = "debug", fields(title = %title))]
pub async fn find_note_by_title(
    db: &SqlitePool,
    title: &str,
) -> Result<Option<NoteRecord>, DatabaseError> {
    sqlx::query_as::<_, NoteRecord>("SELECT * FROM note WHERE title = ? LIMIT 1")
        .bind(title)
        .fetch_optional(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "note",
            operation: "find_by_title",
            source,
        })
}

#[tracing::instrument(skip_all, level = "debug", fields(ref_id = %ref_id))]
pub async fn get_reference(
    db: &SqlitePool,
    ref_id: &str,
) -> Result<ReferenceRecord, DatabaseError> {
    sqlx::query_as::<_, ReferenceRecord>("SELECT * FROM reference WHERE id = ?")
        .bind(ref_id)
        .fetch_optional(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "reference",
            operation: "get",
            source,
        })?
        .ok_or(DatabaseError::MissingRow {
            table: "reference",
            operation: "get",
        })
}

#[tracing::instrument(skip_all, level = "debug", fields(date = %date))]
pub async fn get_diary_by_date(
    db: &SqlitePool,
    date: &str,
) -> Result<Option<DiaryRecord>, DatabaseError> {
    sqlx::query_as::<_, DiaryRecord>("SELECT * FROM diary WHERE date = ? LIMIT 1")
        .bind(date)
        .fetch_optional(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "diary",
            operation: "get_by_date",
            source,
        })
}

#[tracing::instrument(skip_all, level = "debug")]
pub async fn list_recent(db: &SqlitePool, limit: usize) -> Result<Vec<RecentItem>, DatabaseError> {
    let limit_i64 = limit as i64;

    let notes: Vec<RecentItem> = sqlx::query_as(
        "SELECT id, title, 'note' AS kind, updated_at FROM note \
         ORDER BY updated_at DESC LIMIT ?",
    )
    .bind(limit_i64)
    .fetch_all(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "knowledge",
        operation: "list_recent/notes",
        source,
    })?;

    let refs: Vec<RecentItem> = sqlx::query_as(
        "SELECT r.id, COALESCE(t.name, r.topic_id) AS title, 'reference' AS kind, \
         r.created_at AS updated_at FROM reference r \
         LEFT JOIN topic t ON t.id = r.topic_id \
         ORDER BY updated_at DESC LIMIT ?",
    )
    .bind(limit_i64)
    .fetch_all(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "knowledge",
        operation: "list_recent/refs",
        source,
    })?;

    let diary: Vec<RecentItem> = sqlx::query_as(
        "SELECT id, date AS title, 'diary' AS kind, updated_at FROM diary \
         ORDER BY updated_at DESC LIMIT ?",
    )
    .bind(limit_i64)
    .fetch_all(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "knowledge",
        operation: "list_recent/diary",
        source,
    })?;

    let mut all = notes;
    all.extend(refs);
    all.extend(diary);
    all.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    all.truncate(limit);
    Ok(all)
}

// --- Delete ---

#[tracing::instrument(skip_all, level = "debug", fields(note_id = %note_id))]
pub async fn delete_note(db: &SqlitePool, note_id: &str) -> Result<(), DatabaseError> {
    // CASCADE handles relates_to and cited edges via foreign keys
    sqlx::query("DELETE FROM note WHERE id = ?")
        .bind(note_id)
        .execute(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "note",
            operation: "delete",
            source,
        })?;
    Ok(())
}

#[tracing::instrument(skip_all, level = "debug", fields(ref_id = %ref_id))]
pub async fn delete_reference(db: &SqlitePool, ref_id: &str) -> Result<(), DatabaseError> {
    // CASCADE handles cited edges via foreign keys
    sqlx::query("DELETE FROM reference WHERE id = ?")
        .bind(ref_id)
        .execute(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "reference",
            operation: "delete",
            source,
        })?;
    Ok(())
}

#[tracing::instrument(skip_all, level = "debug", fields(path = %path))]
pub async fn find_note_by_path(
    db: &SqlitePool,
    path: &str,
) -> Result<Option<NoteRecord>, DatabaseError> {
    sqlx::query_as::<_, NoteRecord>("SELECT * FROM note WHERE path = ? LIMIT 1")
        .bind(path)
        .fetch_optional(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "note",
            operation: "find_by_path",
            source,
        })
}

// --- Delete diary ---

#[tracing::instrument(skip_all, level = "debug", fields(diary_id = %diary_id))]
pub async fn delete_diary(db: &SqlitePool, diary_id: &str) -> Result<(), DatabaseError> {
    sqlx::query("DELETE FROM diary WHERE id = ?")
        .bind(diary_id)
        .execute(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "diary",
            operation: "delete",
            source,
        })?;
    Ok(())
}

// --- Reference updates ---

#[tracing::instrument(skip_all, level = "debug", fields(ref_id = %ref_id))]
pub async fn update_reference(
    db: &SqlitePool,
    ref_id: &str,
    content: &str,
    file_hash: Option<&str>,
) -> Result<(), DatabaseError> {
    sqlx::query("UPDATE reference SET content = ?, file_hash = ? WHERE id = ?")
        .bind(content)
        .bind(file_hash)
        .bind(ref_id)
        .execute(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "reference",
            operation: "update",
            source,
        })?;
    Ok(())
}

#[tracing::instrument(skip_all, level = "debug", fields(ref_id = %ref_id))]
pub async fn update_reference_path(
    db: &SqlitePool,
    ref_id: &str,
    new_path: &str,
    new_topic_id: &str,
) -> Result<(), DatabaseError> {
    sqlx::query("UPDATE reference SET path = ?, topic_id = ? WHERE id = ?")
        .bind(new_path)
        .bind(new_topic_id)
        .bind(ref_id)
        .execute(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "reference",
            operation: "update_path",
            source,
        })?;
    Ok(())
}

#[tracing::instrument(skip_all, level = "debug", fields(path = %path))]
pub async fn find_reference_by_path(
    db: &SqlitePool,
    path: &str,
) -> Result<Option<ReferenceRecord>, DatabaseError> {
    sqlx::query_as::<_, ReferenceRecord>("SELECT * FROM reference WHERE path = ? LIMIT 1")
        .bind(path)
        .fetch_optional(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "reference",
            operation: "find_by_path",
            source,
        })
}

// --- Reference lookup by URL ---

#[tracing::instrument(skip_all, level = "debug", fields(url = %url))]
pub async fn find_reference_by_url(
    db: &SqlitePool,
    url: &str,
) -> Result<Option<ReferenceRecord>, DatabaseError> {
    sqlx::query_as::<_, ReferenceRecord>("SELECT * FROM reference WHERE source_url = ? LIMIT 1")
        .bind(url)
        .fetch_optional(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "reference",
            operation: "find_by_url",
            source,
        })
}

// --- Reference browsing ---

#[tracing::instrument(skip_all, level = "debug", fields(topic_id = ?topic_id))]
pub async fn list_references_by_topic(
    db: &SqlitePool,
    topic_id: Option<&str>,
    limit: usize,
) -> Result<Vec<ReferenceRecord>, DatabaseError> {
    match topic_id {
        Some(tid) => {
            sqlx::query_as::<_, ReferenceRecord>(
                "SELECT * FROM reference WHERE topic_id = ? \
             ORDER BY created_at DESC LIMIT ?",
            )
            .bind(tid)
            .bind(limit as i64)
            .fetch_all(db)
            .await
        }
        None => {
            sqlx::query_as::<_, ReferenceRecord>(
                "SELECT * FROM reference ORDER BY topic_id, created_at DESC LIMIT ?",
            )
            .bind(limit as i64)
            .fetch_all(db)
            .await
        }
    }
    .map_err(|source| DatabaseError::Query {
        table: "reference",
        operation: "list_by_topic",
        source,
    })
}

// --- Bulk hash loading for boot reconciliation ---

/// Lightweight record for boot reconciliation: just path + hash + embedding status.
#[derive(Debug, sqlx::FromRow)]
pub struct FileHashRecord {
    pub path: String,
    pub file_hash: Option<String>,
    pub has_embeddings: bool,
}

/// Like `FileHashRecord` but for code files, which carry a `repo` dimension.
#[derive(Debug, sqlx::FromRow)]
pub struct CodeFileHashRecord {
    pub repo: String,
    pub path: String,
    pub file_hash: Option<String>,
    pub has_embeddings: bool,
}

/// Load all (path, file_hash, has_embeddings) for notes.
pub async fn load_note_file_hashes(db: &SqlitePool) -> Result<Vec<FileHashRecord>, DatabaseError> {
    sqlx::query_as::<_, FileHashRecord>(
        "SELECT \
            n.path AS path, \
            n.file_hash, \
            (e.source_id IS NOT NULL) AS has_embeddings \
         FROM note n \
         LEFT JOIN ( \
            SELECT DISTINCT source_id FROM embedding WHERE source_table = 'note' \
         ) e ON e.source_id = n.id \
         WHERE n.path IS NOT NULL",
    )
    .fetch_all(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "note",
        operation: "load_file_hashes",
        source,
    })
}

/// Load all (path, file_hash, has_embeddings) for references.
pub async fn load_reference_file_hashes(
    db: &SqlitePool,
) -> Result<Vec<FileHashRecord>, DatabaseError> {
    sqlx::query_as::<_, FileHashRecord>(
        "SELECT \
            r.path AS path, \
            r.file_hash, \
            (e.source_id IS NOT NULL) AS has_embeddings \
         FROM reference r \
         LEFT JOIN ( \
            SELECT DISTINCT source_id FROM embedding WHERE source_table = 'reference' \
         ) e ON e.source_id = r.id",
    )
    .fetch_all(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "reference",
        operation: "load_file_hashes",
        source,
    })
}

/// Load all (date as path, file_hash, has_embeddings) for diary entries.
pub async fn load_diary_file_hashes(db: &SqlitePool) -> Result<Vec<FileHashRecord>, DatabaseError> {
    sqlx::query_as::<_, FileHashRecord>(
        "SELECT \
            d.date AS path, \
            d.file_hash, \
            (e.source_id IS NOT NULL) AS has_embeddings \
         FROM diary d \
         LEFT JOIN ( \
            SELECT DISTINCT source_id FROM embedding WHERE source_table = 'diary' \
         ) e ON e.source_id = d.id",
    )
    .fetch_all(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "diary",
        operation: "load_file_hashes",
        source,
    })
}

/// Load all (path, file_hash, has_embeddings) for scripts.
pub async fn load_script_file_hashes(
    db: &SqlitePool,
) -> Result<Vec<FileHashRecord>, DatabaseError> {
    sqlx::query_as::<_, FileHashRecord>(
        "SELECT \
            s.path AS path, \
            s.file_hash, \
            (e.source_id IS NOT NULL) AS has_embeddings \
         FROM script s \
         LEFT JOIN ( \
            SELECT DISTINCT source_id FROM embedding WHERE source_table = 'script' \
         ) e ON e.source_id = s.id",
    )
    .fetch_all(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "script",
        operation: "load_file_hashes",
        source,
    })
}

/// Load all (repo, path, file_hash, has_embeddings) for code files.
pub async fn load_code_file_hashes(
    db: &SqlitePool,
) -> Result<Vec<CodeFileHashRecord>, DatabaseError> {
    sqlx::query_as::<_, CodeFileHashRecord>(
        "SELECT \
            cf.repo, cf.path, cf.file_hash, \
            (e.source_id IS NOT NULL) AS has_embeddings \
         FROM code_file cf \
         LEFT JOIN ( \
            SELECT DISTINCT source_id FROM embedding WHERE source_table = 'code_file' \
         ) e ON e.source_id = cf.id",
    )
    .fetch_all(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "code_file",
        operation: "load_file_hashes",
        source,
    })
}

// --- Bulk listing for embeddings pipeline ---

pub async fn list_all_notes(db: &SqlitePool) -> Result<Vec<NoteRecord>, DatabaseError> {
    sqlx::query_as::<_, NoteRecord>("SELECT * FROM note")
        .fetch_all(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "note",
            operation: "list_all",
            source,
        })
}

pub async fn list_all_references(db: &SqlitePool) -> Result<Vec<ReferenceRecord>, DatabaseError> {
    sqlx::query_as::<_, ReferenceRecord>("SELECT * FROM reference")
        .fetch_all(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "reference",
            operation: "list_all",
            source,
        })
}

pub async fn list_all_diary(db: &SqlitePool) -> Result<Vec<DiaryRecord>, DatabaseError> {
    sqlx::query_as::<_, DiaryRecord>("SELECT * FROM diary")
        .fetch_all(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "diary",
            operation: "list_all",
            source,
        })
}

// ---------------------------------------------------------------------------
// Scripts
// ---------------------------------------------------------------------------

#[tracing::instrument(skip_all, level = "debug", fields(path = %path))]
pub async fn create_script(
    db: &SqlitePool,
    path: &str,
    content: &str,
    file_hash: Option<&str>,
) -> Result<String, DatabaseError> {
    let id = new_id();
    let ts = now();

    sqlx::query(
        "INSERT INTO script (id, path, content, file_hash, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(path)
    .bind(content)
    .bind(file_hash)
    .bind(&ts)
    .bind(&ts)
    .execute(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "script",
        operation: "create",
        source,
    })?;

    Ok(id)
}

#[tracing::instrument(skip_all, level = "debug", fields(script_id = %script_id))]
pub async fn update_script(
    db: &SqlitePool,
    script_id: &str,
    content: &str,
    file_hash: Option<&str>,
) -> Result<(), DatabaseError> {
    sqlx::query("UPDATE script SET content = ?, file_hash = ?, updated_at = ? WHERE id = ?")
        .bind(content)
        .bind(file_hash)
        .bind(now())
        .bind(script_id)
        .execute(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "script",
            operation: "update",
            source,
        })?;
    Ok(())
}

#[tracing::instrument(skip_all, level = "debug", fields(script_id = %script_id))]
pub async fn get_script(db: &SqlitePool, script_id: &str) -> Result<ScriptRecord, DatabaseError> {
    sqlx::query_as::<_, ScriptRecord>("SELECT * FROM script WHERE id = ?")
        .bind(script_id)
        .fetch_optional(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "script",
            operation: "get",
            source,
        })?
        .ok_or(DatabaseError::MissingRow {
            table: "script",
            operation: "get",
        })
}

#[tracing::instrument(skip_all, level = "debug", fields(path = %path))]
pub async fn find_script_by_path(
    db: &SqlitePool,
    path: &str,
) -> Result<Option<ScriptRecord>, DatabaseError> {
    sqlx::query_as::<_, ScriptRecord>("SELECT * FROM script WHERE path = ? LIMIT 1")
        .bind(path)
        .fetch_optional(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "script",
            operation: "find_by_path",
            source,
        })
}

#[tracing::instrument(skip_all, level = "debug", fields(script_id = %script_id))]
pub async fn delete_script(db: &SqlitePool, script_id: &str) -> Result<(), DatabaseError> {
    sqlx::query("DELETE FROM script WHERE id = ?")
        .bind(script_id)
        .execute(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "script",
            operation: "delete",
            source,
        })?;
    Ok(())
}

pub async fn list_all_scripts(db: &SqlitePool) -> Result<Vec<ScriptRecord>, DatabaseError> {
    sqlx::query_as::<_, ScriptRecord>("SELECT * FROM script")
        .fetch_all(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "script",
            operation: "list_all",
            source,
        })
}

// ---------------------------------------------------------------------------
// Code Files
// ---------------------------------------------------------------------------

#[tracing::instrument(skip_all, level = "debug", fields(repo = %repo, path = %path))]
pub async fn create_code_file(
    db: &SqlitePool,
    repo: &str,
    path: &str,
    content: &str,
    file_hash: Option<&str>,
) -> Result<String, DatabaseError> {
    let id = new_id();
    let ts = now();
    sqlx::query(
        "INSERT INTO code_file (id, repo, path, content, file_hash, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(repo)
    .bind(path)
    .bind(content)
    .bind(file_hash)
    .bind(&ts)
    .bind(&ts)
    .execute(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "code_file",
        operation: "create",
        source,
    })?;
    Ok(id)
}

#[tracing::instrument(skip_all, level = "debug", fields(code_file_id = %code_file_id))]
pub async fn update_code_file(
    db: &SqlitePool,
    code_file_id: &str,
    content: &str,
    file_hash: Option<&str>,
) -> Result<(), DatabaseError> {
    sqlx::query("UPDATE code_file SET content = ?, file_hash = ?, updated_at = ? WHERE id = ?")
        .bind(content)
        .bind(file_hash)
        .bind(now())
        .bind(code_file_id)
        .execute(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "code_file",
            operation: "update",
            source,
        })?;
    Ok(())
}

pub async fn find_code_file(
    db: &SqlitePool,
    repo: &str,
    path: &str,
) -> Result<Option<CodeFileRecord>, DatabaseError> {
    sqlx::query_as::<_, CodeFileRecord>(
        "SELECT * FROM code_file WHERE repo = ? AND path = ? LIMIT 1",
    )
    .bind(repo)
    .bind(path)
    .fetch_optional(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "code_file",
        operation: "find",
        source,
    })
}

pub async fn delete_code_file(db: &SqlitePool, code_file_id: &str) -> Result<(), DatabaseError> {
    sqlx::query("DELETE FROM code_file WHERE id = ?")
        .bind(code_file_id)
        .execute(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "code_file",
            operation: "delete",
            source,
        })?;
    Ok(())
}

pub async fn delete_code_files_by_repo(db: &SqlitePool, repo: &str) -> Result<u64, DatabaseError> {
    let result = sqlx::query("DELETE FROM code_file WHERE repo = ?")
        .bind(repo)
        .execute(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "code_file",
            operation: "delete_by_repo",
            source,
        })?;
    Ok(result.rows_affected())
}
