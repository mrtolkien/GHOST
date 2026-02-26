use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;
use tracing::info;

use crate::config::EmbeddingsConfig;
use crate::db::GhostDb;
use crate::embeddings::EmbeddingClient;
use crate::embeddings::pipeline::PipelineError;
use crate::knowledge;

/// Spawn the file watcher. Returns a `JoinHandle` that runs until the
/// shutdown signal is received.
#[tracing::instrument(name = "start watcher", skip_all)]
pub fn spawn_watcher(
    db: GhostDb,
    workspace: PathBuf,
    embeddings_config: EmbeddingsConfig,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let client = EmbeddingClient::new(&embeddings_config);

        if !client.is_available().await {
            logfire::warn!("Ollama unavailable at watcher start — file watcher disabled");
            // Wait for shutdown instead of polling
            let _ = shutdown.changed().await;
            return;
        }

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

            for path in &changed_paths {
                let kind = classify_watcher_kind(&workspace, path);
                let _span = logfire::span!(
                    "process file_change",
                    kind = kind,
                    path = path.display().to_string(),
                );
                if let Err(e) = process_change(&db, &workspace, &client, path).await {
                    logfire::warn!(
                        "embedding watcher error",
                        path = path.display().to_string(),
                        error = e.to_string(),
                    );
                }
            }
        }

        info!("file watcher stopped");
    })
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

    for dir in [&notes_dir, &refs_dir, &diary_dir] {
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
    } else {
        "unknown"
    }
}

async fn process_change(
    db: &GhostDb,
    workspace: &Path,
    client: &EmbeddingClient,
    path: &Path,
) -> Result<(), PipelineError> {
    // Determine the kind of knowledge item from the path
    let rel = match path.strip_prefix(workspace) {
        Ok(r) => r,
        Err(_) => return Ok(()),
    };

    let rel_str = rel.to_string_lossy();

    if rel_str.starts_with("notes/") {
        process_note_change(db, workspace, client, path).await
    } else if rel_str.starts_with("references/") {
        process_reference_change(db, workspace, client, path).await
    } else if rel_str.starts_with("diary/") {
        process_diary_change(db, client, path).await
    } else {
        Ok(())
    }
}

/// Sync a changed note file to the database and regenerate its embeddings.
///
/// If the note already exists in the DB (matched by title), updates it in place.
/// Otherwise creates a new DB record. Also reconciles wiki-link edges.
async fn process_note_change(
    db: &GhostDb,
    _workspace: &Path,
    client: &EmbeddingClient,
    path: &Path,
) -> Result<(), PipelineError> {
    if !path.exists() {
        // File removed — we'd need to know the source_id to delete
        // embeddings. Without DB lookup by path, skip.
        return Ok(());
    }

    let raw = match std::fs::read_to_string(path) {
        Ok(r) => r,
        Err(_) => return Ok(()),
    };

    let parsed = match knowledge::parse_note(&raw) {
        Ok(p) => p,
        Err(_) => return Ok(()),
    };

    let filename = path
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or_default()
        .to_string();
    logfire::info!("watcher: processing note change", filename = filename,);

    // Look up the note in DB by title
    let note = match crate::db::knowledge::find_note_by_title(db, &parsed.front.title).await {
        Ok(Some(n)) => n,
        _ => {
            // Not in DB yet — reindex will handle it.
            // But let's try to upsert it first.
            let archetype_str = parsed.front.archetype.map(|a| a.to_string());
            match crate::db::knowledge::create_note_full(
                db,
                &parsed.front.title,
                &parsed.body,
                archetype_str.as_deref(),
                &parsed.front.tags,
                &parsed.front.sources,
                parsed.front.trust,
                None,
            )
            .await
            {
                Ok(note_id) => {
                    // Reconcile edges
                    let _ = knowledge::reconcile::reconcile_edges(
                        db,
                        &note_id,
                        &parsed.front.title,
                        &parsed.wiki_links,
                    )
                    .await;

                    crate::embeddings::pipeline::embed_source(
                        client,
                        db,
                        "note",
                        &note_id,
                        &parsed.body,
                        &parsed.front.tags,
                    )
                    .await?;
                    return Ok(());
                }
                Err(_) => return Ok(()),
            }
        }
    };

    // Update existing note in DB
    let archetype_str = parsed.front.archetype.map(|a| a.to_string());
    let _ = crate::db::knowledge::update_note(
        db,
        &note.id,
        &parsed.body,
        archetype_str.as_deref(),
        &parsed.front.tags,
        &parsed.front.sources,
        parsed.front.trust,
        None,
    )
    .await;

    let _ = knowledge::reconcile::reconcile_edges(
        db,
        &note.id,
        &parsed.front.title,
        &parsed.wiki_links,
    )
    .await;

    crate::embeddings::pipeline::embed_source(
        client,
        db,
        "note",
        &note.id,
        &parsed.body,
        &parsed.front.tags,
    )
    .await?;

    Ok(())
}

/// Sync a changed reference file to the database and regenerate its embeddings.
async fn process_reference_change(
    db: &GhostDb,
    workspace: &Path,
    client: &EmbeddingClient,
    path: &Path,
) -> Result<(), PipelineError> {
    if !path.exists() {
        return Ok(());
    }

    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return Ok(()),
    };

    let rel_path = path
        .strip_prefix(workspace)
        .unwrap_or(path)
        .to_string_lossy()
        .to_string();

    let topic = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|f| f.to_str())
        .unwrap_or("unknown")
        .to_string();

    logfire::info!(
        "watcher: processing reference change",
        path = rel_path.clone(),
    );

    let reference = match crate::db::knowledge::find_reference_by_path(db, &rel_path).await {
        Ok(Some(r)) => r,
        _ => {
            match crate::db::knowledge::create_reference(db, &topic, &rel_path, &content, None)
                .await
            {
                Ok(ref_id) => {
                    crate::embeddings::pipeline::embed_source(
                        client,
                        db,
                        "reference",
                        &ref_id,
                        &content,
                        &[],
                    )
                    .await?;
                    return Ok(());
                }
                Err(_) => return Ok(()),
            }
        }
    };

    crate::embeddings::pipeline::embed_source(
        client,
        db,
        "reference",
        &reference.id,
        &content,
        &[],
    )
    .await?;

    Ok(())
}

async fn process_diary_change(
    db: &GhostDb,
    client: &EmbeddingClient,
    path: &Path,
) -> Result<(), PipelineError> {
    if !path.exists() {
        return Ok(());
    }

    let body = match std::fs::read_to_string(path) {
        Ok(b) => b,
        Err(_) => return Ok(()),
    };

    let date = path
        .file_stem()
        .and_then(|f| f.to_str())
        .unwrap_or("unknown")
        .to_string();

    logfire::info!("watcher: processing diary change", date = date.clone(),);

    let entry = match crate::db::knowledge::get_diary_by_date(db, &date).await {
        Ok(Some(d)) => d,
        _ => match crate::db::knowledge::create_diary(db, &date, &body).await {
            Ok(diary_id) => {
                crate::embeddings::pipeline::embed_source(
                    client,
                    db,
                    "diary",
                    &diary_id,
                    &body,
                    &[],
                )
                .await?;
                return Ok(());
            }
            Err(_) => return Ok(()),
        },
    };

    crate::embeddings::pipeline::embed_source(client, db, "diary", &entry.id, &body, &[]).await?;

    Ok(())
}
