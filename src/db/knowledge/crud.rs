use surrealdb::Surreal;
use surrealdb::engine::local::Db;
use surrealdb::sql::Thing;

use crate::db::error::DatabaseError;
use crate::db::query::{IdRow, query_exec, take_many, take_one};

use super::records::{DiaryRecord, NoteRecord, RecentItem, ReferenceRecord};

// --- Create / Update ---

#[tracing::instrument(skip_all, level = "debug", fields(title = %title))]
pub async fn create_note(
    db: &Surreal<Db>,
    title: &str,
    body: &str,
) -> Result<Thing, DatabaseError> {
    create_note_full(db, title, body, None, &[], 5).await
}

#[tracing::instrument(skip_all, level = "debug", fields(title = %title))]
pub async fn create_note_full(
    db: &Surreal<Db>,
    title: &str,
    body: &str,
    archetype: Option<&str>,
    tags: &[String],
    trust: i64,
) -> Result<Thing, DatabaseError> {
    let mut resp = query_exec(
        db.query(
            "CREATE note SET \
                title = $title, \
                body = $body, \
                archetype = $archetype, \
                tags = $tags, \
                trust = $trust, \
                created_at = time::now(), \
                updated_at = time::now() \
             RETURN id",
        )
        .bind(("title", title.to_string()))
        .bind(("body", body.to_string()))
        .bind(("archetype", archetype.map(ToString::to_string)))
        .bind(("tags", tags.to_vec()))
        .bind(("trust", trust)),
        "note",
        "create",
    )
    .await?;

    let row: IdRow = take_one(&mut resp, 0, "note", "create")?;
    Ok(row.id)
}

#[tracing::instrument(skip_all, level = "debug", fields(note_id = %note_id))]
pub async fn update_note(
    db: &Surreal<Db>,
    note_id: &Thing,
    body: &str,
    archetype: Option<&str>,
    tags: &[String],
    trust: i64,
) -> Result<(), DatabaseError> {
    query_exec(
        db.query(
            "UPDATE $note_id SET \
                body = $body, \
                archetype = $archetype, \
                tags = $tags, \
                trust = $trust, \
                updated_at = time::now()",
        )
        .bind(("note_id", note_id.clone()))
        .bind(("body", body.to_string()))
        .bind(("archetype", archetype.map(ToString::to_string)))
        .bind(("tags", tags.to_vec()))
        .bind(("trust", trust)),
        "note",
        "update",
    )
    .await?;
    Ok(())
}

#[tracing::instrument(skip_all, level = "debug", fields(date = %date))]
pub async fn create_diary(
    db: &Surreal<Db>,
    date: &str,
    body: &str,
) -> Result<Thing, DatabaseError> {
    let mut resp = query_exec(
        db.query(
            "CREATE diary SET \
                date = $date, \
                body = $body, \
                updated_at = time::now() \
             RETURN id",
        )
        .bind(("date", date.to_string()))
        .bind(("body", body.to_string())),
        "diary",
        "create",
    )
    .await?;

    let row: IdRow = take_one(&mut resp, 0, "diary", "create")?;
    Ok(row.id)
}

#[tracing::instrument(skip_all, level = "debug", fields(date = %date))]
pub async fn append_diary(db: &Surreal<Db>, date: &str, line: &str) -> Result<(), DatabaseError> {
    query_exec(
        db.query(
            "UPDATE diary SET \
                body = string::concat(body, '\\n', $line), \
                updated_at = time::now() \
             WHERE date = $date",
        )
        .bind(("date", date.to_string()))
        .bind(("line", line.to_string())),
        "diary",
        "append",
    )
    .await?;
    Ok(())
}

#[tracing::instrument(skip_all, level = "debug", fields(topic = %topic, path = %path))]
pub async fn create_reference(
    db: &Surreal<Db>,
    topic: &str,
    path: &str,
    content: &str,
    source_url: Option<&str>,
) -> Result<Thing, DatabaseError> {
    let mut resp = query_exec(
        db.query(
            "CREATE reference SET \
                topic = $topic, \
                path = $path, \
                content = $content, \
                source_url = $source_url, \
                created_at = time::now() \
             RETURN id",
        )
        .bind(("topic", topic.to_string()))
        .bind(("path", path.to_string()))
        .bind(("content", content.to_string()))
        .bind(("source_url", source_url.map(ToString::to_string))),
        "reference",
        "create",
    )
    .await?;

    let row: IdRow = take_one(&mut resp, 0, "reference", "create")?;
    Ok(row.id)
}

// --- Read ---

#[tracing::instrument(skip_all, level = "debug", fields(note_id = %note_id))]
pub async fn get_note(db: &Surreal<Db>, note_id: &Thing) -> Result<NoteRecord, DatabaseError> {
    let mut resp = query_exec(
        db.query("SELECT * FROM ONLY $note_id")
            .bind(("note_id", note_id.clone())),
        "note",
        "get",
    )
    .await?;

    crate::db::query::take_opt(&mut resp, 0, "note", "get")?.ok_or(DatabaseError::MissingRow {
        table: "note",
        operation: "get",
    })
}

#[tracing::instrument(skip_all, level = "debug", fields(title = %title))]
pub async fn find_note_by_title(
    db: &Surreal<Db>,
    title: &str,
) -> Result<Option<NoteRecord>, DatabaseError> {
    let mut resp = query_exec(
        db.query("SELECT * FROM note WHERE title = $title LIMIT 1")
            .bind(("title", title.to_string())),
        "note",
        "find_by_title",
    )
    .await?;

    let rows: Vec<NoteRecord> = take_many(&mut resp, 0, "note", "find_by_title")?;
    Ok(rows.into_iter().next())
}

#[tracing::instrument(skip_all, level = "debug", fields(ref_id = %ref_id))]
pub async fn get_reference(
    db: &Surreal<Db>,
    ref_id: &Thing,
) -> Result<ReferenceRecord, DatabaseError> {
    let mut resp = query_exec(
        db.query("SELECT * FROM ONLY $ref_id")
            .bind(("ref_id", ref_id.clone())),
        "reference",
        "get",
    )
    .await?;

    crate::db::query::take_opt(&mut resp, 0, "reference", "get")?.ok_or(DatabaseError::MissingRow {
        table: "reference",
        operation: "get",
    })
}

#[tracing::instrument(skip_all, level = "debug", fields(date = %date))]
pub async fn get_diary_by_date(
    db: &Surreal<Db>,
    date: &str,
) -> Result<Option<DiaryRecord>, DatabaseError> {
    let mut resp = query_exec(
        db.query("SELECT * FROM diary WHERE date = $date LIMIT 1")
            .bind(("date", date.to_string())),
        "diary",
        "get_by_date",
    )
    .await?;

    let rows: Vec<DiaryRecord> = take_many(&mut resp, 0, "diary", "get_by_date")?;
    Ok(rows.into_iter().next())
}

#[tracing::instrument(skip_all, level = "debug")]
pub async fn list_recent(db: &Surreal<Db>, limit: usize) -> Result<Vec<RecentItem>, DatabaseError> {
    let mut resp = query_exec(
        db.query(
            "SELECT id, title, 'note' AS kind, updated_at FROM note \
             ORDER BY updated_at DESC LIMIT $limit; \
             SELECT id, topic AS title, 'reference' AS kind, created_at AS updated_at FROM reference \
             ORDER BY updated_at DESC LIMIT $limit; \
             SELECT id, date AS title, 'diary' AS kind, updated_at FROM diary \
             ORDER BY updated_at DESC LIMIT $limit;",
        )
        .bind(("limit", limit as i64)),
        "knowledge",
        "list_recent",
    )
    .await?;

    let notes: Vec<RecentItem> = take_many(&mut resp, 0, "knowledge", "list_recent/notes")?;
    let refs: Vec<RecentItem> = take_many(&mut resp, 1, "knowledge", "list_recent/refs")?;
    let diary: Vec<RecentItem> = take_many(&mut resp, 2, "knowledge", "list_recent/diary")?;

    let mut all = notes;
    all.extend(refs);
    all.extend(diary);
    all.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    all.truncate(limit);
    Ok(all)
}

// --- Delete ---

#[tracing::instrument(skip_all, level = "debug", fields(note_id = %note_id))]
pub async fn delete_note(db: &Surreal<Db>, note_id: &Thing) -> Result<(), DatabaseError> {
    query_exec(
        db.query(
            "DELETE relates_to WHERE `in` = $note_id OR out = $note_id; \
             DELETE cited WHERE `in` = $note_id OR out = $note_id; \
             DELETE $note_id",
        )
        .bind(("note_id", note_id.clone())),
        "note",
        "delete",
    )
    .await?;
    Ok(())
}

#[tracing::instrument(skip_all, level = "debug", fields(ref_id = %ref_id))]
pub async fn delete_reference(db: &Surreal<Db>, ref_id: &Thing) -> Result<(), DatabaseError> {
    query_exec(
        db.query(
            "DELETE cited WHERE `in` = $ref_id OR out = $ref_id; \
             DELETE $ref_id",
        )
        .bind(("ref_id", ref_id.clone())),
        "reference",
        "delete",
    )
    .await?;
    Ok(())
}

// --- Reference updates ---

#[tracing::instrument(skip_all, level = "debug", fields(ref_id = %ref_id))]
pub async fn update_reference_path(
    db: &Surreal<Db>,
    ref_id: &Thing,
    new_path: &str,
    new_topic: &str,
) -> Result<(), DatabaseError> {
    query_exec(
        db.query("UPDATE $ref_id SET path = $path, topic = $topic")
            .bind(("ref_id", ref_id.clone()))
            .bind(("path", new_path.to_string()))
            .bind(("topic", new_topic.to_string())),
        "reference",
        "update_path",
    )
    .await?;
    Ok(())
}

#[tracing::instrument(skip_all, level = "debug", fields(path = %path))]
pub async fn find_reference_by_path(
    db: &Surreal<Db>,
    path: &str,
) -> Result<Option<ReferenceRecord>, DatabaseError> {
    let mut resp = query_exec(
        db.query("SELECT * FROM reference WHERE path = $path LIMIT 1")
            .bind(("path", path.to_string())),
        "reference",
        "find_by_path",
    )
    .await?;

    let rows: Vec<ReferenceRecord> = take_many(&mut resp, 0, "reference", "find_by_path")?;
    Ok(rows.into_iter().next())
}

// --- Bulk listing for embeddings pipeline ---

pub async fn list_all_notes(db: &Surreal<Db>) -> Result<Vec<NoteRecord>, DatabaseError> {
    let mut resp = query_exec(db.query("SELECT * FROM note"), "note", "list_all").await?;
    take_many(&mut resp, 0, "note", "list_all")
}

pub async fn list_all_references(db: &Surreal<Db>) -> Result<Vec<ReferenceRecord>, DatabaseError> {
    let mut resp = query_exec(db.query("SELECT * FROM reference"), "reference", "list_all").await?;
    take_many(&mut resp, 0, "reference", "list_all")
}

pub async fn list_all_diary(db: &Surreal<Db>) -> Result<Vec<DiaryRecord>, DatabaseError> {
    let mut resp = query_exec(db.query("SELECT * FROM diary"), "diary", "list_all").await?;
    take_many(&mut resp, 0, "diary", "list_all")
}
