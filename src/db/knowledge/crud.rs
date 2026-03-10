use sqlx::SqlitePool;

use crate::db::error::DatabaseError;
use crate::db::{new_id, now};

use super::records::{DiaryRecord, NoteRecord, RecentItem, ReferenceRecord, ScriptRecord};

// --- Create / Update ---

#[tracing::instrument(skip_all, level = "debug", fields(title = %title))]
pub async fn create_note(
    db: &SqlitePool,
    title: &str,
    body: &str,
) -> Result<String, DatabaseError> {
    create_note_full(db, title, body, &[], &[], 5, None, None).await
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
    topic_id: Option<&str>,
    path: Option<&str>,
) -> Result<String, DatabaseError> {
    let id = new_id();
    let ts = now();
    let tags_json = serde_json::to_string(tags).unwrap_or_default();
    let sources_json = serde_json::to_string(sources).unwrap_or_default();

    sqlx::query(
        "INSERT INTO note \
         (id, title, body, tags, sources, trust, topic_id, path, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(title)
    .bind(body)
    .bind(&tags_json)
    .bind(&sources_json)
    .bind(trust)
    .bind(topic_id)
    .bind(path)
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
    topic_id: Option<&str>,
    path: Option<&str>,
) -> Result<(), DatabaseError> {
    let tags_json = serde_json::to_string(tags).unwrap_or_default();
    let sources_json = serde_json::to_string(sources).unwrap_or_default();

    sqlx::query(
        "UPDATE note SET body = ?, tags = ?, sources = ?, \
         trust = ?, topic_id = COALESCE(?, topic_id), path = ?, updated_at = ? WHERE id = ?",
    )
    .bind(body)
    .bind(&tags_json)
    .bind(&sources_json)
    .bind(trust)
    .bind(topic_id)
    .bind(path)
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
) -> Result<String, DatabaseError> {
    let id = new_id();

    sqlx::query("INSERT INTO diary (id, date, body, updated_at) VALUES (?, ?, ?, ?)")
        .bind(&id)
        .bind(date)
        .bind(body)
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
) -> Result<String, DatabaseError> {
    let id = new_id();
    let ts = now();

    // Upsert: if a reference with the same (topic_id, path) already exists,
    // update its content in place. This handles re-imports gracefully — e.g.
    // when a long-running PDF import is retried after a timeout.
    let row = sqlx::query_as::<_, (String,)>(
        "INSERT INTO reference (id, topic_id, path, content, source_url, import_batch_id, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?) \
         ON CONFLICT(topic_id, path) DO UPDATE SET \
           content = excluded.content, \
           source_url = excluded.source_url, \
           import_batch_id = excluded.import_batch_id \
         RETURNING id",
    )
    .bind(&id)
    .bind(topic_id)
    .bind(path)
    .bind(content)
    .bind(source_url)
    .bind(import_batch_id)
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

// --- Paginated listing for boot reconciliation ---

pub async fn list_notes_page(
    db: &SqlitePool,
    offset: usize,
    limit: usize,
) -> Result<Vec<NoteRecord>, DatabaseError> {
    sqlx::query_as::<_, NoteRecord>("SELECT * FROM note ORDER BY id LIMIT ? OFFSET ?")
        .bind(limit as i64)
        .bind(offset as i64)
        .fetch_all(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "note",
            operation: "list_page",
            source,
        })
}

pub async fn list_references_page(
    db: &SqlitePool,
    offset: usize,
    limit: usize,
) -> Result<Vec<ReferenceRecord>, DatabaseError> {
    sqlx::query_as::<_, ReferenceRecord>("SELECT * FROM reference ORDER BY id LIMIT ? OFFSET ?")
        .bind(limit as i64)
        .bind(offset as i64)
        .fetch_all(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "reference",
            operation: "list_page",
            source,
        })
}

pub async fn list_diary_page(
    db: &SqlitePool,
    offset: usize,
    limit: usize,
) -> Result<Vec<DiaryRecord>, DatabaseError> {
    sqlx::query_as::<_, DiaryRecord>("SELECT * FROM diary ORDER BY id LIMIT ? OFFSET ?")
        .bind(limit as i64)
        .bind(offset as i64)
        .fetch_all(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "diary",
            operation: "list_page",
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
) -> Result<String, DatabaseError> {
    let id = new_id();
    let ts = now();

    sqlx::query(
        "INSERT INTO script (id, path, content, created_at, updated_at) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(path)
    .bind(content)
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
) -> Result<(), DatabaseError> {
    sqlx::query("UPDATE script SET content = ?, updated_at = ? WHERE id = ?")
        .bind(content)
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

pub async fn list_scripts_page(
    db: &SqlitePool,
    offset: usize,
    limit: usize,
) -> Result<Vec<ScriptRecord>, DatabaseError> {
    sqlx::query_as::<_, ScriptRecord>("SELECT * FROM script ORDER BY id LIMIT ? OFFSET ?")
        .bind(limit as i64)
        .bind(offset as i64)
        .fetch_all(db)
        .await
        .map_err(|source| DatabaseError::Query {
            table: "script",
            operation: "list_page",
            source,
        })
}
