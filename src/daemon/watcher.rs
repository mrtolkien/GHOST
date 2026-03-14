use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;
use tracing::{Instrument, info};

use crate::config::EmbeddingsConfig;
use crate::db::GhostDb;
use crate::embeddings::EmbeddingClient;
use crate::embeddings::pipeline::{EmbedRequest, PipelineError};
use crate::knowledge;

/// Spawn the file watcher. Returns a `JoinHandle` that runs until the
/// shutdown signal is received.
#[tracing::instrument(name = "start watcher", skip_all)]
pub fn spawn_watcher(
    db: GhostDb,
    workspace: PathBuf,
    embeddings_config: EmbeddingsConfig,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
    watcher_busy: Arc<AtomicBool>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let client = EmbeddingClient::new(&embeddings_config);

        let (tx, mut rx) = mpsc::channel::<PathBuf>(256);

        let _watcher = match setup_watcher(&workspace, tx) {
            Ok(w) => w,
            Err(e) => {
                logfire::error!("failed to start file watcher", error = e.to_string(),);
                return;
            }
        };

        info!("file watcher started");

        // Debounce: collect events for 500ms before processing
        let debounce = Duration::from_millis(500);

        loop {
            let mut changed_paths: HashSet<PathBuf> = HashSet::new();

            tokio::select! {
                path = rx.recv() => {
                    match path {
                        Some(p) => { changed_paths.insert(p); }
                        None => break,
                    }
                }
                _ = shutdown.changed() => break,
            }

            // Drain additional events within the debounce window
            tokio::time::sleep(debounce).await;
            while let Ok(path) = rx.try_recv() {
                changed_paths.insert(path);
            }

            watcher_busy.store(true, Ordering::Relaxed);
            process_batch(&db, &workspace, &client, &changed_paths).await;
            watcher_busy.store(false, Ordering::Relaxed);
        }

        info!("file watcher stopped");
    })
}

#[tracing::instrument(name = "process file_changes", skip_all, fields(count = paths.len()))]
async fn process_batch(
    db: &GhostDb,
    workspace: &Path,
    client: &EmbeddingClient,
    paths: &HashSet<PathBuf>,
) {
    let paths = expand_directories(paths);

    // Phase 1: process each file (DB upserts, deletions) and collect embed requests
    let mut embed_requests: Vec<EmbedRequest> = Vec::new();

    for path in &paths {
        let kind = classify_watcher_kind(workspace, path);
        let req = async {
            match process_change(db, workspace, path).await {
                Ok(req) => req,
                Err(e) => {
                    logfire::warn!(
                        "embedding watcher error",
                        path = path.display().to_string(),
                        error = e.to_string(),
                    );
                    None
                }
            }
        }
        .instrument(logfire::span!(
            "process file_change",
            kind = kind,
            path = path.display().to_string(),
        ))
        .await;

        if let Some(r) = req {
            embed_requests.push(r);
        }
    }

    // Phase 2: batch-embed all collected sources (skip if Ollama unavailable)
    if !embed_requests.is_empty() {
        if client.is_available().await {
            if let Err(e) =
                crate::embeddings::pipeline::embed_sources(client, db, embed_requests).await
            {
                logfire::warn!("batch embedding error", error = e.to_string());
            }
        } else {
            logfire::debug!(
                "Ollama unavailable — skipping embedding (will catch up on reconciliation)",
                sources = embed_requests.len(),
            );
        }
    }
}

fn setup_watcher(
    workspace: &Path,
    tx: mpsc::Sender<PathBuf>,
) -> Result<RecommendedWatcher, notify::Error> {
    let mut watcher = notify::recommended_watcher(move |result: Result<Event, _>| {
        if let Ok(event) = result {
            match event.kind {
                EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) => {
                    for path in event.paths {
                        let _ = tx.try_send(path);
                    }
                }
                _ => {}
            }
        }
    })?;

    let notes_dir = workspace.join("notes");
    let refs_dir = workspace.join("references");
    let diary_dir = workspace.join("diary");
    let scripts_dir = workspace.join("scripts");

    for dir in [&notes_dir, &refs_dir, &diary_dir, &scripts_dir] {
        if dir.exists() {
            watcher.watch(dir, RecursiveMode::Recursive)?;
        }
    }

    Ok(watcher)
}

fn classify_watcher_kind(workspace: &Path, path: &Path) -> &'static str {
    let rel = path
        .strip_prefix(workspace)
        .map(|r| r.to_string_lossy())
        .unwrap_or_default();
    if rel.starts_with("notes/") {
        "note"
    } else if rel.starts_with("references/") {
        "reference"
    } else if rel.starts_with("diary/") {
        "diary"
    } else if rel.starts_with("scripts/") {
        "script"
    } else {
        "unknown"
    }
}

/// Expand directory paths into their contained files.
///
/// When inotify reports a directory creation, files written into it
/// before the watch was established are missed. By expanding directory
/// paths, we catch those files.
fn expand_directories(paths: &HashSet<PathBuf>) -> HashSet<PathBuf> {
    let mut result = HashSet::new();
    for path in paths {
        if path.is_dir() {
            collect_files_recursive(path, &mut result);
        } else {
            result.insert(path.clone());
        }
    }
    result
}

fn collect_files_recursive(dir: &Path, out: &mut HashSet<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files_recursive(&path, out);
        } else if path.is_file() {
            out.insert(path);
        }
    }
}

pub(crate) async fn process_change(
    db: &GhostDb,
    workspace: &Path,
    path: &Path,
) -> Result<Option<EmbedRequest>, PipelineError> {
    let rel = match path.strip_prefix(workspace) {
        Ok(r) => r,
        Err(_) => return Ok(None),
    };

    let rel_str = rel.to_string_lossy();

    if rel_str.starts_with("notes/") {
        process_note_change(db, workspace, path).await
    } else if rel_str.starts_with("references/") {
        process_reference_change(db, workspace, path).await
    } else if rel_str.starts_with("diary/") {
        process_diary_change(db, path).await
    } else if rel_str.starts_with("scripts/") {
        process_script_change(db, workspace, path).await
    } else {
        Ok(None)
    }
}

/// Sync a changed note file to the database.
///
/// Returns an `EmbedRequest` if the note needs (re-)embedding.
/// When the file has been deleted, removes the DB record and its embeddings.
async fn process_note_change(
    db: &GhostDb,
    workspace: &Path,
    path: &Path,
) -> Result<Option<EmbedRequest>, PipelineError> {
    let rel_path = path
        .strip_prefix(workspace)
        .unwrap_or(path)
        .to_string_lossy()
        .to_string();

    if !path.exists() {
        if let Ok(Some(note)) = crate::db::knowledge::find_note_by_path(db, &rel_path).await {
            crate::db::embeddings::delete_embeddings_for_source(db, &note.id).await?;
            crate::db::knowledge::delete_note(db, &note.id).await?;
            logfire::info!("watcher: deleted note", path = rel_path);
        }
        return Ok(None);
    }

    let raw = match std::fs::read_to_string(path) {
        Ok(r) => r,
        Err(_) => return Ok(None),
    };

    let parsed = match knowledge::parse_note(&raw) {
        Ok(p) => p,
        Err(_) => return Ok(None),
    };

    let filename = path
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or_default()
        .to_string();
    logfire::info!("watcher: processing note change", filename = filename,);

    // Resolve topic_id from subfolder path (e.g. "notes/dioxus/foo.md" → topic "dioxus")
    let topic_id = {
        let without_notes = rel_path.strip_prefix("notes/").unwrap_or(&rel_path);
        match without_notes.rsplit_once('/') {
            Some((folder, _)) if !folder.is_empty() => {
                crate::db::knowledge::find_topic_by_name(db, folder)
                    .await
                    .ok()
                    .flatten()
                    .map(|t| t.id)
            }
            _ => None,
        }
    };

    // Look up the note in DB by title
    let note_id = match crate::db::knowledge::find_note_by_title(db, &parsed.front.title).await {
        Ok(Some(n)) => {
            // Update existing note
            let _ = crate::db::knowledge::update_note(
                db,
                &n.id,
                &parsed.body,
                &parsed.front.tags,
                &parsed.front.sources,
                parsed.front.trust,
                topic_id.as_deref(),
                Some(&rel_path),
                None,
            )
            .await;
            let _ = knowledge::reconcile::reconcile_edges(
                db,
                &n.id,
                &parsed.front.title,
                &parsed.wiki_links,
            )
            .await;
            n.id
        }
        _ => {
            match crate::db::knowledge::create_note_full(
                db,
                &parsed.front.title,
                &parsed.body,
                &parsed.front.tags,
                &parsed.front.sources,
                parsed.front.trust,
                topic_id.as_deref(),
                Some(&rel_path),
                None,
            )
            .await
            {
                Ok(id) => {
                    let _ = knowledge::reconcile::reconcile_edges(
                        db,
                        &id,
                        &parsed.front.title,
                        &parsed.wiki_links,
                    )
                    .await;
                    id
                }
                Err(_) => return Ok(None),
            }
        }
    };

    Ok(Some(EmbedRequest {
        source_table: "note".into(),
        source_id: note_id,
        content: parsed.body,
        tags: parsed.front.tags,
        topic_id,
        path: None,
    }))
}

/// Sync a changed reference file to the database.
///
/// Returns an `EmbedRequest` if the reference needs (re-)embedding.
/// When the file has been deleted, removes the DB record and its embeddings.
async fn process_reference_change(
    db: &GhostDb,
    workspace: &Path,
    path: &Path,
) -> Result<Option<EmbedRequest>, PipelineError> {
    let fs_rel = path
        .strip_prefix(workspace)
        .unwrap_or(path)
        .to_string_lossy()
        .to_string();

    // DB stores paths without the `references/` prefix (e.g. `ark-nova/rules/slug.md`).
    // The filesystem-relative path includes it, so strip it for DB lookups/inserts.
    let ref_path = fs_rel
        .strip_prefix("references/")
        .unwrap_or(&fs_rel)
        .to_string();

    if !path.exists() {
        if let Ok(Some(ref_)) = crate::db::knowledge::find_reference_by_path(db, &ref_path).await {
            crate::db::embeddings::delete_embeddings_for_source(db, &ref_.id).await?;
            crate::db::knowledge::delete_reference(db, &ref_.id).await?;
            logfire::info!("watcher: deleted reference", path = ref_path);
        }
        return Ok(None);
    }

    // Ignore _import.toml files
    if path.file_name().and_then(|f| f.to_str()) == Some("_import.toml") {
        return Ok(None);
    }

    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return Ok(None),
    };

    // Extract full topic path: everything before the filename
    // e.g. `ark-nova/rules/slug.md` → `ark-nova/rules`
    let topic_name = match ref_path.rsplit_once('/') {
        Some((parent, _)) => parent.to_string(),
        None => "unknown".to_string(),
    };

    logfire::info!(
        "watcher: processing reference change",
        path = ref_path.clone(),
    );

    let (ref_id, resolved_topic_id) =
        match crate::db::knowledge::find_reference_by_path(db, &ref_path).await {
            Ok(Some(r)) => (r.id.clone(), r.topic_id.clone()),
            _ => {
                let tid = match crate::db::knowledge::find_or_create_topic(db, &topic_name).await {
                    Ok(id) => id,
                    Err(_) => return Ok(None),
                };
                match crate::db::knowledge::create_reference(
                    db, &tid, &ref_path, &content, None, None, None,
                )
                .await
                {
                    Ok(id) => (id, tid),
                    Err(_) => return Ok(None),
                }
            }
        };

    // Use filesystem-relative path (with references/ prefix) for code chunking
    let embed_path = fs_rel;

    Ok(Some(EmbedRequest {
        source_table: "reference".into(),
        source_id: ref_id,
        content,
        tags: vec![],
        topic_id: Some(resolved_topic_id),
        path: Some(embed_path),
    }))
}

/// Sync a changed diary file to the database.
///
/// Returns an `EmbedRequest` if the diary entry needs (re-)embedding.
/// When the file has been deleted, removes the DB record and its embeddings.
async fn process_diary_change(
    db: &GhostDb,
    path: &Path,
) -> Result<Option<EmbedRequest>, PipelineError> {
    let date = path
        .file_stem()
        .and_then(|f| f.to_str())
        .unwrap_or("unknown")
        .to_string();

    if !path.exists() {
        if let Ok(Some(diary)) = crate::db::knowledge::get_diary_by_date(db, &date).await {
            crate::db::embeddings::delete_embeddings_for_source(db, &diary.id).await?;
            crate::db::knowledge::delete_diary(db, &diary.id).await?;
            logfire::info!("watcher: deleted diary", date = date);
        }
        return Ok(None);
    }

    let body = match std::fs::read_to_string(path) {
        Ok(b) => b,
        Err(_) => return Ok(None),
    };

    logfire::info!("watcher: processing diary change", date = date.clone(),);

    let diary_id = match crate::db::knowledge::get_diary_by_date(db, &date).await {
        Ok(Some(d)) => d.id,
        _ => match crate::db::knowledge::create_diary(db, &date, &body, None).await {
            Ok(id) => id,
            Err(_) => return Ok(None),
        },
    };

    Ok(Some(EmbedRequest {
        source_table: "diary".into(),
        source_id: diary_id,
        content: body,
        tags: vec![],
        topic_id: None,
        path: None,
    }))
}

/// Sync a changed script file to the database.
///
/// Scripts are simpler than notes: no frontmatter, no wiki links.
/// The file content IS the script. Path is relative to workspace
/// (e.g. `scripts/finance/spending.py`).
async fn process_script_change(
    db: &GhostDb,
    workspace: &Path,
    path: &Path,
) -> Result<Option<EmbedRequest>, PipelineError> {
    let fs_rel = path
        .strip_prefix(workspace)
        .unwrap_or(path)
        .to_string_lossy()
        .to_string();

    // DB stores path without the leading "scripts/" prefix
    let script_path = fs_rel
        .strip_prefix("scripts/")
        .unwrap_or(&fs_rel)
        .to_string();

    // Deletion
    if !path.exists() {
        if let Ok(Some(script)) = crate::db::knowledge::find_script_by_path(db, &script_path).await
        {
            crate::db::embeddings::delete_embeddings_for_source(db, &script.id).await?;
            crate::db::knowledge::delete_script(db, &script.id).await?;
            logfire::info!("watcher: deleted script", path = script_path);
        }
        return Ok(None);
    }

    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return Ok(None),
    };

    let script_id = match crate::db::knowledge::find_script_by_path(db, &script_path).await {
        Ok(Some(s)) => {
            let _ = crate::db::knowledge::update_script(db, &s.id, &content, None).await;
            s.id
        }
        _ => match crate::db::knowledge::create_script(db, &script_path, &content, None).await {
            Ok(id) => id,
            Err(_) => return Ok(None),
        },
    };

    Ok(Some(EmbedRequest {
        source_table: "script".into(),
        source_id: script_id,
        content,
        tags: vec![],
        topic_id: None,
        path: Some(fs_rel),
    }))
}

const RECONCILE_INTERVAL: Duration = Duration::from_secs(60 * 60);

/// Periodically reconcile embeddings to catch missed file changes.
///
/// Runs `reconcile_embeddings` once per hour. Skips if Ollama is unavailable.
/// The hash check inside `reconcile_embeddings` makes this cheap when nothing changed.
#[tracing::instrument(name = "start reconciliation loop", skip_all)]
pub fn spawn_reconciliation_loop(
    db: GhostDb,
    workspace: PathBuf,
    embeddings_config: EmbeddingsConfig,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let client = EmbeddingClient::new(&embeddings_config);

        loop {
            tokio::select! {
                _ = tokio::time::sleep(RECONCILE_INTERVAL) => {}
                _ = shutdown.changed() => break,
            }

            if !client.is_available().await {
                logfire::debug!("Ollama unavailable — skipping periodic reconciliation");
                continue;
            }

            async {
                // Phase 1: discover files on disk that the watcher missed
                match crate::embeddings::pipeline::reconcile_filesystem(&db, &workspace).await {
                    Ok(discovered) if discovered > 0 => {
                        info!(discovered, "filesystem reconciliation found new files");
                    }
                    Err(e) => {
                        logfire::warn!("filesystem reconciliation failed", error = e.to_string());
                    }
                    _ => {}
                }

                // Phase 2: re-embed any sources with stale content hashes
                match crate::embeddings::pipeline::reconcile_embeddings(&client, &db).await {
                    Ok((embedded, skipped)) => {
                        if embedded > 0 {
                            info!(embedded, skipped, "periodic reconciliation complete");
                        }
                    }
                    Err(e) => {
                        logfire::warn!("periodic reconciliation failed", error = e.to_string(),);
                    }
                }
            }
            .instrument(tracing::info_span!("reconcile periodic"))
            .await;
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_directories_finds_nested_files() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("topic").join("subtopic");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("file.md"), "content").unwrap();
        std::fs::write(sub.join("_import.toml"), "meta").unwrap();
        std::fs::write(dir.path().join("root.md"), "root").unwrap();

        let mut paths = HashSet::new();
        paths.insert(dir.path().join("topic"));
        paths.insert(dir.path().join("root.md"));

        let expanded = expand_directories(&paths);

        assert!(expanded.contains(&sub.join("file.md")));
        assert!(expanded.contains(&sub.join("_import.toml")));
        assert!(expanded.contains(&dir.path().join("root.md")));
        assert!(!expanded.contains(&dir.path().join("topic")));
    }

    #[test]
    fn expand_directories_handles_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let empty = dir.path().join("empty");
        std::fs::create_dir_all(&empty).unwrap();

        let mut paths = HashSet::new();
        paths.insert(empty.clone());

        let expanded = expand_directories(&paths);
        assert!(expanded.is_empty());
    }
}
