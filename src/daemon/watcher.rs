use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;
use tracing::{Instrument, info};

use crate::config::{SharedConfig, SharedConfigExt};
use crate::db::GhostDb;
use crate::embeddings::EmbeddingClient;
use crate::embeddings::pipeline::{EmbedReason, EmbedRequest, PipelineError};
use crate::knowledge;

const WATCHER_CHANNEL_BUFFER: usize = 256;
const DEBOUNCE_INTERVAL: Duration = Duration::from_millis(500);

/// Spawn the file watcher. Returns a `JoinHandle` that runs until the
/// shutdown signal is received.
#[tracing::instrument(name = "start watcher", skip_all)]
pub fn spawn_watcher(
    db: GhostDb,
    config: SharedConfig,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
    watcher_busy: Arc<AtomicBool>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let cfg = config.current();
        let workspace = cfg.workspace.clone();
        let client = EmbeddingClient::new(&cfg.embeddings);

        let (tx, mut rx) = mpsc::channel::<PathBuf>(WATCHER_CHANNEL_BUFFER);

        let _watcher = match setup_watcher(&workspace, tx) {
            Ok(w) => w,
            Err(e) => {
                tracing::error!(error = e.to_string(), "failed to start file watcher");
                return;
            }
        };

        info!("file watcher started");

        let debounce = DEBOUNCE_INTERVAL;

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
            // Read file and compute hash upfront so it's stored with the DB record
            let (content, hash) = if path.exists() {
                match tokio::fs::read_to_string(path).await {
                    Ok(raw) => {
                        let h = crate::embeddings::pipeline::content_hash(&raw);
                        (Some(raw), Some(h))
                    }
                    Err(_) => (None, None),
                }
            } else {
                (None, None)
            };

            match process_change(db, workspace, path, content.as_deref(), hash.as_deref()).await {
                Ok(req) => req,
                Err(e) => {
                    tracing::warn!(
                        path = path.display().to_string(),
                        error = e.to_string(),
                        "embedding watcher error",
                    );
                    None
                }
            }
        }
        .instrument(tracing::info_span!(
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
                tracing::warn!(error = e.to_string(), "batch embedding error");
            }
        } else {
            tracing::debug!(
                sources = embed_requests.len(),
                "Ollama unavailable — skipping embedding (will catch up on reconciliation)",
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
    let code_dir = workspace.join("code");

    for dir in [&notes_dir, &refs_dir, &diary_dir, &scripts_dir, &code_dir] {
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
    } else if rel.starts_with("code/") {
        "code"
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
    raw_content: Option<&str>,
    file_hash: Option<&str>,
) -> Result<Option<EmbedRequest>, PipelineError> {
    let Ok(rel) = path.strip_prefix(workspace) else {
        return Ok(None);
    };

    let rel_str = rel.to_string_lossy();

    if rel_str.starts_with("notes/") {
        process_note_change(db, workspace, path, raw_content, file_hash).await
    } else if rel_str.starts_with("references/") {
        process_reference_change(db, workspace, path, raw_content, file_hash).await
    } else if rel_str.starts_with("diary/") {
        process_diary_change(db, path, raw_content, file_hash).await
    } else if rel_str.starts_with("scripts/") {
        process_script_change(db, workspace, path, raw_content, file_hash).await
    } else if rel_str.starts_with("code/") {
        process_code_file_change(db, workspace, path, raw_content, file_hash).await
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
    raw_content: Option<&str>,
    file_hash: Option<&str>,
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
            tracing::info!(path = rel_path, "watcher: deleted note");
        }
        return Ok(None);
    }

    let owned;
    let raw = match raw_content {
        Some(c) => c,
        None => {
            owned = match tokio::fs::read_to_string(path).await {
                Ok(r) => r,
                Err(_) => return Ok(None),
            };
            &owned
        }
    };

    let Ok(parsed) = knowledge::parse_note(raw) else {
        return Ok(None);
    };

    let filename = path
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or_default()
        .to_string();
    tracing::info!(filename = filename, "watcher: processing note change");

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

    // Extract archetype from parsed frontmatter for DB storage
    let archetype_str = parsed.front.archetype.to_string();

    // Look up the note in DB by title
    let note_input = crate::db::knowledge::NoteInput {
        title: &parsed.front.title,
        body: &parsed.body,
        tags: &parsed.front.tags,
        sources: &parsed.front.sources,
        trust: parsed.front.trust,
        archetype: Some(archetype_str.as_str()),
        topic_id: topic_id.as_deref(),
        path: Some(&rel_path),
        file_hash,
    };

    let note_id = match crate::db::knowledge::find_note_by_title(db, &parsed.front.title).await {
        Ok(Some(n)) => {
            // Update existing note
            if let Err(e) = crate::db::knowledge::update_note(db, &n.id, &note_input).await {
                tracing::warn!(note_id = %n.id, error = %e, "failed to update note from file watcher");
            }
            if let Err(e) = knowledge::reconcile::reconcile_edges(
                db,
                &n.id,
                &parsed.front.title,
                &parsed.wiki_links,
                parsed.front.parent.as_deref(),
            )
            .await
            {
                tracing::warn!(note_id = %n.id, error = %e, "failed to reconcile edges");
            }
            n.id
        }
        _ => match crate::db::knowledge::create_note_full(db, &note_input).await {
            Ok(id) => {
                if let Err(e) = knowledge::reconcile::reconcile_edges(
                    db,
                    &id,
                    &parsed.front.title,
                    &parsed.wiki_links,
                    parsed.front.parent.as_deref(),
                )
                .await
                {
                    tracing::warn!(note_id = %id, error = %e, "failed to reconcile edges");
                }
                id
            }
            Err(_) => return Ok(None),
        },
    };

    Ok(Some(EmbedRequest {
        source_table: "note".into(),
        source_id: note_id,
        content: parsed.body,
        tags: parsed.front.tags,
        topic_id,
        path: None,
        reason: EmbedReason::Changed,
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
    raw_content: Option<&str>,
    file_hash: Option<&str>,
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
            tracing::info!(path = ref_path, "watcher: deleted reference");
        }
        return Ok(None);
    }

    // Ignore _import.toml files
    if path.file_name().and_then(|f| f.to_str()) == Some("_import.toml") {
        return Ok(None);
    }

    let owned;
    let content = match raw_content {
        Some(c) => c,
        None => {
            owned = match tokio::fs::read_to_string(path).await {
                Ok(c) => c,
                Err(_) => return Ok(None),
            };
            &owned
        }
    };

    // Extract full topic path: everything before the filename
    // e.g. `ark-nova/rules/slug.md` → `ark-nova/rules`
    let topic_name = match ref_path.rsplit_once('/') {
        Some((parent, _)) => parent.to_string(),
        None => "unknown".to_string(),
    };

    tracing::info!(
        path = ref_path.clone(),
        "watcher: processing reference change",
    );

    let (ref_id, resolved_topic_id) =
        match crate::db::knowledge::find_reference_by_path(db, &ref_path).await {
            Ok(Some(r)) => {
                let _ = crate::db::knowledge::update_reference(db, &r.id, content, file_hash).await;
                (r.id, r.topic_id)
            }
            _ => {
                let Ok(tid) = crate::db::knowledge::find_or_create_topic(db, &topic_name).await
                else {
                    return Ok(None);
                };
                match crate::db::knowledge::create_reference(
                    db, &tid, &ref_path, content, None, None, file_hash,
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
        content: content.to_string(),
        tags: vec![],
        topic_id: Some(resolved_topic_id),
        path: Some(embed_path),
        reason: EmbedReason::Changed,
    }))
}

/// Sync a changed diary file to the database.
///
/// Returns an `EmbedRequest` if the diary entry needs (re-)embedding.
/// When the file has been deleted, removes the DB record and its embeddings.
async fn process_diary_change(
    db: &GhostDb,
    path: &Path,
    raw_content: Option<&str>,
    file_hash: Option<&str>,
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
            tracing::info!(date = date, "watcher: deleted diary");
        }
        return Ok(None);
    }

    let owned;
    let body = match raw_content {
        Some(c) => c,
        None => {
            owned = match tokio::fs::read_to_string(path).await {
                Ok(b) => b,
                Err(_) => return Ok(None),
            };
            &owned
        }
    };

    tracing::info!(date = date.clone(), "watcher: processing diary change");

    let diary_id = match crate::db::knowledge::get_diary_by_date(db, &date).await {
        Ok(Some(d)) => {
            let _ = crate::db::knowledge::update_diary(db, &d.id, body, file_hash).await;
            d.id
        }
        _ => match crate::db::knowledge::create_diary(db, &date, body, file_hash).await {
            Ok(id) => id,
            Err(_) => return Ok(None),
        },
    };

    Ok(Some(EmbedRequest {
        source_table: "diary".into(),
        source_id: diary_id,
        content: body.to_string(),
        tags: vec![],
        topic_id: None,
        path: None,
        reason: EmbedReason::Changed,
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
    raw_content: Option<&str>,
    file_hash: Option<&str>,
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
            tracing::info!(path = script_path, "watcher: deleted script");
        }
        return Ok(None);
    }

    let owned;
    let content = match raw_content {
        Some(c) => c,
        None => {
            owned = match tokio::fs::read_to_string(path).await {
                Ok(c) => c,
                Err(_) => return Ok(None),
            };
            &owned
        }
    };

    let script_id = match crate::db::knowledge::find_script_by_path(db, &script_path).await {
        Ok(Some(s)) => {
            let _ = crate::db::knowledge::update_script(db, &s.id, content, file_hash).await;
            s.id
        }
        _ => {
            match crate::db::knowledge::create_script(db, &script_path, content, file_hash).await {
                Ok(id) => id,
                Err(_) => return Ok(None),
            }
        }
    };

    Ok(Some(EmbedRequest {
        source_table: "script".into(),
        source_id: script_id,
        content: content.to_string(),
        tags: vec![],
        topic_id: None,
        path: Some(fs_rel),
        reason: EmbedReason::Changed,
    }))
}

/// Sync a changed code file to the database.
///
/// Similar to `process_script_change` but with repo-scoped paths,
/// extension allowlist, and size guard. Creates a synthetic topic
/// `"code/<repo>"` for vector search scoping.
async fn process_code_file_change(
    db: &GhostDb,
    workspace: &Path,
    path: &Path,
    raw_content: Option<&str>,
    file_hash: Option<&str>,
) -> Result<Option<EmbedRequest>, PipelineError> {
    let Some(repo) = crate::embeddings::pipeline::extract_repo_slug(workspace, path) else {
        return Ok(None);
    };

    let fs_rel = path
        .strip_prefix(workspace)
        .unwrap_or(path)
        .to_string_lossy()
        .to_string();

    // DB stores path relative to repo root (strip "code/<slug>/")
    let code_path = fs_rel
        .strip_prefix(&format!("code/{repo}/"))
        .unwrap_or(&fs_rel)
        .to_string();

    // Extension check
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    if !crate::embeddings::pipeline::CODE_EXTENSIONS.contains(&ext) {
        return Ok(None);
    }

    // Size guard
    if path
        .metadata()
        .is_ok_and(|m| m.len() > crate::embeddings::pipeline::MAX_CODE_FILE_SIZE)
    {
        return Ok(None);
    }

    // Deletion
    if !path.exists() {
        if let Ok(Some(cf)) = crate::db::knowledge::find_code_file(db, &repo, &code_path).await {
            crate::db::embeddings::delete_embeddings_for_source(db, &cf.id).await?;
            crate::db::knowledge::delete_code_file(db, &cf.id).await?;
            tracing::info!(repo = repo, path = code_path, "watcher: deleted code file");
        }
        return Ok(None);
    }

    let owned;
    let content = match raw_content {
        Some(c) => c,
        None => {
            owned = match tokio::fs::read_to_string(path).await {
                Ok(c) => c,
                Err(_) => return Ok(None),
            };
            &owned
        }
    };

    // Find or create synthetic topic for this repo
    let topic_name = format!("code/{repo}");
    let topic_id = crate::db::knowledge::find_or_create_topic(db, &topic_name).await?;

    let code_file_id = match crate::db::knowledge::find_code_file(db, &repo, &code_path).await {
        Ok(Some(cf)) => {
            let _ = crate::db::knowledge::update_code_file(db, &cf.id, content, file_hash).await;
            cf.id
        }
        _ => {
            match crate::db::knowledge::create_code_file(db, &repo, &code_path, content, file_hash)
                .await
            {
                Ok(id) => id,
                Err(_) => return Ok(None),
            }
        }
    };

    Ok(Some(EmbedRequest {
        source_table: "code_file".into(),
        source_id: code_file_id,
        content: content.to_string(),
        tags: vec![],
        topic_id: Some(topic_id),
        path: Some(fs_rel),
        reason: EmbedReason::Changed,
    }))
}

const RECONCILE_INTERVAL: Duration = Duration::from_secs(60 * 60);

/// Periodically reconcile filesystem and embeddings to catch missed changes.
///
/// Runs once per hour. Skips if Ollama is unavailable.
/// The file-hash check makes this cheap when nothing changed.
#[tracing::instrument(name = "start reconciliation loop", skip_all)]
pub fn spawn_reconciliation_loop(
    db: GhostDb,
    config: SharedConfig,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let cfg = config.current();
        let workspace = cfg.workspace.clone();
        let client = EmbeddingClient::new(&cfg.embeddings);

        loop {
            tokio::select! {
                _ = tokio::time::sleep(RECONCILE_INTERVAL) => {}
                _ = shutdown.changed() => break,
            }

            if !client.is_available().await {
                tracing::debug!("Ollama unavailable — skipping periodic reconciliation");
                continue;
            }

            Box::pin(
                async {
                    match Box::pin(crate::embeddings::pipeline::reconcile_filesystem(
                        &db, &workspace,
                    ))
                    .await
                    {
                        Ok((discovered, embed_requests)) => {
                            if discovered > 0 {
                                info!(discovered, "periodic reconciliation found new files");
                            }
                            if !embed_requests.is_empty() {
                                match crate::embeddings::pipeline::embed_sources(
                                    &client,
                                    &db,
                                    embed_requests,
                                )
                                .await
                                {
                                    Ok(embedded) if embedded > 0 => {
                                        info!(embedded, "periodic reconciliation complete");
                                    }
                                    Err(e) => {
                                        tracing::warn!(
                                            error = e.to_string(),
                                            "periodic embedding failed",
                                        );
                                    }
                                    _ => {}
                                }
                            }
                        }
                        Err(e) => {
                            tracing::warn!(error = e.to_string(), "periodic reconciliation failed");
                        }
                    }
                }
                .instrument(tracing::trace_span!("reconcile periodic")),
            )
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
