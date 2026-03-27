use clap::Subcommand;

use crate::db;
use crate::db::GhostDb;
use crate::db::knowledge::NoteInput;
use crate::embeddings::EmbeddingClient;
use crate::error::GhostError;
use crate::knowledge;

#[derive(Debug, Subcommand)]
pub enum KnowledgeCommand {
    Search {
        query: String,
        #[arg(long)]
        kind: Option<String>,
        #[arg(long)]
        topic: Option<String>,
        #[arg(long, default_value_t = 10)]
        limit: usize,
    },
    Get {
        path: Option<String>,
        #[arg(long)]
        title: Option<String>,
    },
    Graph {
        target: String,
        #[arg(long)]
        direction: Option<String>,
        #[arg(long)]
        orphans: bool,
        #[arg(long)]
        stats: bool,
    },
    Tags,
    Recent {
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    Stats,
    References {
        #[arg(long)]
        topic: Option<String>,
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    Reindex {
        #[arg(long)]
        skip_embeddings: bool,
    },
}

#[tracing::instrument(skip_all)]
pub async fn execute(command: KnowledgeCommand) -> Result<(), GhostError> {
    let config = crate::config::load()?;
    crate::config_workspace::bootstrap_workspace(&config)?;
    let db = crate::db::connect(&config.workspace, config.embeddings.dimension).await?;

    match command {
        KnowledgeCommand::Search {
            query,
            kind,
            topic,
            limit,
        } => {
            cmd_search(
                &db,
                &config.embeddings,
                &query,
                kind.as_deref(),
                topic.as_deref(),
                limit,
            )
            .await
        }
        KnowledgeCommand::Get { path, title } => {
            cmd_get(&db, &config.workspace, path.as_deref(), title.as_deref()).await
        }
        KnowledgeCommand::Graph {
            target,
            direction,
            orphans,
            stats,
        } => cmd_graph(&db, &target, direction.as_deref(), orphans, stats).await,
        KnowledgeCommand::Tags => cmd_tags(&db).await,
        KnowledgeCommand::Recent { limit } => cmd_recent(&db, limit).await,
        KnowledgeCommand::Stats => cmd_stats(&db).await,
        KnowledgeCommand::References { topic, limit } => {
            cmd_references(&db, topic.as_deref(), limit).await
        }
        KnowledgeCommand::Reindex { skip_embeddings } => {
            cmd_reindex(&db, &config.workspace, &config.embeddings, skip_embeddings).await
        }
    }
}

/// Run a hybrid search (BM25 + vector) across notes, references, and diary.
async fn cmd_search(
    db: &GhostDb,
    embeddings_config: &crate::config::EmbeddingsConfig,
    query: &str,
    kind: Option<&str>,
    topic: Option<&str>,
    limit: usize,
) -> Result<(), GhostError> {
    // Resolve topic name to topic IDs for scoped search
    let resolved_topic_ids = if let Some(topic_name) = topic {
        db::knowledge::find_topics_by_prefix(db, topic_name)
            .await?
            .into_iter()
            .map(|t| t.id)
            .collect::<Vec<_>>()
    } else {
        vec![]
    };

    let mut bm25_hits = Vec::new();

    if kind.is_none() || kind == Some("note") {
        bm25_hits.extend(db::knowledge::search_notes(db, query, limit, None).await?);
    }
    if kind.is_none() || kind == Some("reference") {
        if resolved_topic_ids.is_empty() {
            bm25_hits.extend(db::knowledge::search_references(db, query, limit, None).await?);
        } else {
            for tid in &resolved_topic_ids {
                bm25_hits
                    .extend(db::knowledge::search_references(db, query, limit, Some(tid)).await?);
            }
        }
    }
    if kind.is_none() || kind == Some("diary") {
        bm25_hits.extend(db::knowledge::search_diary(db, query, limit).await?);
    }

    // Try hybrid search: embed query and merge with BM25
    let client = EmbeddingClient::new(embeddings_config);
    let hits = if client.is_available().await {
        match client.embed_batch(&[query.to_string()]).await {
            Ok(vectors) if !vectors.is_empty() => {
                let embedding_hits =
                    db::embeddings::vector_search(db, &vectors[0], limit, &resolved_topic_ids)
                        .await?;
                db::knowledge::hybrid_merge(&bm25_hits, &embedding_hits, limit)
            }
            Ok(_) => {
                tracing::warn!("embedding returned empty vectors, falling back to BM25");
                fallback_bm25(bm25_hits, limit)
            }
            Err(e) => {
                tracing::warn!(
                    error = e.to_string(),
                    "embedding query failed, falling back to BM25",
                );
                fallback_bm25(bm25_hits, limit)
            }
        }
    } else {
        fallback_bm25(bm25_hits, limit)
    };

    if hits.is_empty() {
        println!("No results for '{query}'");
        return Ok(());
    }

    for hit in &hits {
        println!(
            "{:.3}  [{:>9}]  {}  {}",
            hit.score, hit.kind, hit.title, hit.snippet,
        );
    }

    Ok(())
}

fn fallback_bm25(
    mut hits: Vec<db::knowledge::SearchHit>,
    limit: usize,
) -> Vec<db::knowledge::SearchHit> {
    hits.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    hits.truncate(limit);
    hits
}

async fn cmd_get(
    db: &GhostDb,
    workspace: &std::path::Path,
    path: Option<&str>,
    title: Option<&str>,
) -> Result<(), GhostError> {
    if let Some(title) = title {
        let note = db::knowledge::find_note_by_title(db, title)
            .await?
            .ok_or_else(|| GhostError::NotYetImplemented {
                command: "note not found",
            })?;

        println!("ID: {}", note.id);
        println!("Title: {}", note.title);
        let tags = note.tags_parsed();
        if !tags.is_empty() {
            println!("Tags: {}", tags.join(", "));
        }
        println!("Trust: {}", note.trust);
        println!("---");
        println!("{}", note.body);
        return Ok(());
    }

    if let Some(path) = path {
        if path.contains("notes/") {
            let filename = std::path::Path::new(path)
                .file_name()
                .and_then(|f| f.to_str())
                .unwrap_or(path);
            match knowledge::read_note(workspace, filename) {
                Ok(parsed) => {
                    println!("Title: {}", parsed.front.title);
                    if !parsed.front.tags.is_empty() {
                        println!("Tags: {}", parsed.front.tags.join(", "));
                    }
                    println!("Trust: {}", parsed.front.trust);
                    println!("---");
                    println!("{}", parsed.body);
                }
                Err(e) => {
                    eprintln!("Failed to read note: {e}");
                }
            }
        } else {
            let full_path = workspace.join(path);
            let content = std::fs::read_to_string(&full_path)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::NotFound, e))?;
            println!("{content}");
        }
        return Ok(());
    }

    eprintln!("Provide --title or a path argument");
    Ok(())
}

/// Display the knowledge graph around a note: outgoing/incoming edges,
/// orphan detection, and edge/stub statistics.
async fn cmd_graph(
    db: &GhostDb,
    target: &str,
    direction: Option<&str>,
    orphans: bool,
    stats: bool,
) -> Result<(), GhostError> {
    if orphans {
        let orphan_list = db::knowledge::orphan_notes(db).await?;
        if orphan_list.is_empty() {
            println!("No orphan notes");
        } else {
            println!("Orphan notes ({}):", orphan_list.len());
            for note in &orphan_list {
                println!("  {} (trust={})", note.title, note.trust);
            }
        }
        return Ok(());
    }

    if stats {
        let edges = db::knowledge::count_edges(db).await?;
        let stubs = db::knowledge::count_stubs(db).await?;
        println!("Edges: {edges}");
        println!("Stubs: {stubs}");
        return Ok(());
    }

    let note = db::knowledge::find_note_by_title(db, target)
        .await?
        .ok_or_else(|| GhostError::NotYetImplemented {
            command: "note not found for graph",
        })?;

    let show_out = direction.is_none() || direction == Some("out");
    let show_in = direction.is_none() || direction == Some("in");

    println!("Graph for: {} ({})", note.title, note.id);

    if show_out {
        let outgoing = db::knowledge::outgoing_edges(db, &note.id).await?;
        if outgoing.is_empty() {
            println!("  -> (no outgoing edges)");
        } else {
            for edge in &outgoing {
                let target_title = get_note_title(db, &edge.to_id).await;
                println!("  -[{}]-> {}", edge.label, target_title);
            }
        }
    }

    if show_in {
        let incoming = db::knowledge::incoming_edges(db, &note.id).await?;
        if incoming.is_empty() {
            println!("  <- (no incoming edges)");
        } else {
            for edge in &incoming {
                let source_title = get_note_title(db, &edge.from_id).await;
                println!("  <-[{}]- {}", edge.label, source_title);
            }
        }
    }

    let cited = db::knowledge::incoming_cited(db, &note.id).await?;
    if !cited.is_empty() {
        println!("  Cited by: {} message(s)", cited.len());
    }

    Ok(())
}

async fn cmd_tags(db: &GhostDb) -> Result<(), GhostError> {
    let tags = db::knowledge::list_tags_with_counts(db).await?;
    if tags.is_empty() {
        println!("No tags");
        return Ok(());
    }
    for (tag, count) in &tags {
        println!("{tag} ({count})");
    }
    Ok(())
}

async fn cmd_recent(db: &GhostDb, limit: usize) -> Result<(), GhostError> {
    let items = db::knowledge::list_recent(db, limit).await?;
    if items.is_empty() {
        println!("No knowledge items yet");
        return Ok(());
    }
    for item in &items {
        let short_date: String = item.updated_at.chars().take(10).collect();
        println!("{}  [{:>9}]  {}", short_date, item.kind, item.title,);
    }
    Ok(())
}

async fn cmd_stats(db: &GhostDb) -> Result<(), GhostError> {
    let notes = db::knowledge::count_notes(db).await?;
    let stubs = db::knowledge::count_stubs(db).await?;
    let references = db::knowledge::count_references(db).await?;
    let diary = db::knowledge::count_diary(db).await?;
    let edges = db::knowledge::count_edges(db).await?;
    let tags = db::knowledge::list_tags_with_counts(db).await?;
    let embeddings = db::embeddings::count_embeddings(db).await?;

    println!("Notes:      {notes} ({stubs} stubs)");
    println!("References: {references}");
    println!("Diary:      {diary}");
    println!("Edges:      {edges}");
    println!("Tags:       {}", tags.len());
    println!("Embeddings: {embeddings}");
    Ok(())
}

async fn cmd_references(db: &GhostDb, topic: Option<&str>, limit: usize) -> Result<(), GhostError> {
    // Resolve topic name → ID(s) using prefix matching
    let topic_id = if let Some(name) = topic {
        let topics = db::knowledge::find_topics_by_prefix(db, name).await?;
        if topics.is_empty() {
            println!("No references for topic '{name}'");
            return Ok(());
        }
        Some(topics.into_iter().map(|t| t.id).collect::<Vec<_>>())
    } else {
        None
    };

    let refs = match &topic_id {
        Some(ids) => {
            let mut all = Vec::new();
            for id in ids {
                all.extend(db::knowledge::list_references_by_topic(db, Some(id), limit).await?);
            }
            all
        }
        None => db::knowledge::list_references_by_topic(db, None, limit).await?,
    };

    if refs.is_empty() {
        match topic {
            Some(t) => println!("No references for topic '{t}'"),
            None => println!("No references"),
        }
        return Ok(());
    }

    // Build topic_id → name lookup for display
    let all_topics = db::knowledge::list_topics(db).await?;
    let topic_names: std::collections::HashMap<&str, &str> = all_topics
        .iter()
        .map(|t| (t.id.as_str(), t.name.as_str()))
        .collect();

    let mut current_topic_id: Option<&str> = None;
    for r in &refs {
        if current_topic_id != Some(&r.topic_id) {
            if current_topic_id.is_some() {
                println!();
            }
            let fallback = r.topic_id.as_str();
            let display_name = topic_names
                .get(r.topic_id.as_str())
                .copied()
                .unwrap_or(fallback);
            println!("## {display_name}");
            current_topic_id = Some(&r.topic_id);
        }
        let url = r.source_url.as_deref().unwrap_or("-");
        println!("  {}  ({})", r.path, url);
    }

    Ok(())
}

/// Re-sync all workspace knowledge files (notes, references, diary) into the
/// database, reconcile wiki-link edges, and optionally regenerate all embeddings.
async fn cmd_reindex(
    db: &GhostDb,
    workspace: &std::path::Path,
    embeddings_config: &crate::config::EmbeddingsConfig,
    skip_embeddings: bool,
) -> Result<(), GhostError> {
    let mut synced = 0usize;
    let mut created = 0usize;

    // Sync notes
    let note_files =
        knowledge::list_notes(workspace).map_err(|e| std::io::Error::other(e.to_string()))?;

    for path in &note_files {
        let raw = std::fs::read_to_string(path).map_err(std::io::Error::other)?;
        let hash = crate::embeddings::pipeline::content_hash(&raw);
        let parsed = match knowledge::parse_note(&raw) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("Skipping {}: {e}", path.display());
                continue;
            }
        };

        match db::knowledge::find_note_by_title(db, &parsed.front.title).await? {
            Some(existing) => {
                // Compute relative path from file's position
                let rel_path = path
                    .strip_prefix(workspace)
                    .ok()
                    .and_then(|p| p.to_str())
                    .map(std::string::ToString::to_string);
                db::knowledge::update_note(
                    db,
                    &existing.id,
                    &NoteInput {
                        title: &parsed.front.title,
                        body: &parsed.body,
                        tags: &parsed.front.tags,
                        sources: &parsed.front.sources,
                        trust: parsed.front.trust,
                        path: rel_path.as_deref(),
                        file_hash: Some(&hash),
                        ..Default::default()
                    },
                )
                .await?;
                let _result = knowledge::reconcile::reconcile_edges(
                    db,
                    &existing.id,
                    &parsed.front.title,
                    &parsed.wiki_links,
                    parsed.front.parent.as_deref(),
                )
                .await
                .map_err(|e| std::io::Error::other(e.to_string()))?;
                synced += 1;
            }
            None => {
                let rel_path = path
                    .strip_prefix(workspace)
                    .ok()
                    .and_then(|p| p.to_str())
                    .map(std::string::ToString::to_string);
                let note_id = db::knowledge::create_note_full(
                    db,
                    &NoteInput {
                        title: &parsed.front.title,
                        body: &parsed.body,
                        tags: &parsed.front.tags,
                        sources: &parsed.front.sources,
                        trust: parsed.front.trust,
                        path: rel_path.as_deref(),
                        file_hash: Some(&hash),
                        ..Default::default()
                    },
                )
                .await?;
                let _result = knowledge::reconcile::reconcile_edges(
                    db,
                    &note_id,
                    &parsed.front.title,
                    &parsed.wiki_links,
                    parsed.front.parent.as_deref(),
                )
                .await
                .map_err(|e| std::io::Error::other(e.to_string()))?;
                created += 1;
            }
        }
    }

    // Sync references
    let ref_files =
        knowledge::list_references(workspace).map_err(|e| std::io::Error::other(e.to_string()))?;
    let mut ref_synced = 0usize;

    for path in &ref_files {
        // DB stores paths without the `references/` prefix (e.g. `ark-nova/rules/slug.md`).
        let ref_path = path
            .strip_prefix(workspace)
            .unwrap_or(path)
            .to_string_lossy()
            .strip_prefix("references/")
            .unwrap_or(&path.to_string_lossy())
            .to_string();

        // Extract full topic hierarchy: everything before the filename
        // e.g. `ark-nova/rules/slug.md` → `ark-nova/rules`
        let topic_name = ref_path
            .rsplit_once('/')
            .map(|(parent, _)| parent)
            .unwrap_or("unknown");

        let content = std::fs::read_to_string(path).map_err(std::io::Error::other)?;
        let hash = crate::embeddings::pipeline::content_hash(&content);
        if let Some(existing) = db::knowledge::find_reference_by_path(db, &ref_path).await? {
            db::knowledge::update_reference(db, &existing.id, &content, Some(&hash)).await?;
        } else {
            let topic_id = db::knowledge::find_or_create_topic(db, topic_name).await?;
            db::knowledge::create_reference(
                db,
                &topic_id,
                &ref_path,
                &content,
                None,
                None,
                Some(&hash),
            )
            .await?;
        }
        ref_synced += 1;
    }

    // Sync diary entries
    let diary_files = knowledge::list_diary_entries(workspace)
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    let mut diary_synced = 0usize;

    for path in &diary_files {
        let date = path
            .file_stem()
            .and_then(|f| f.to_str())
            .unwrap_or("unknown");
        let body = std::fs::read_to_string(path).map_err(std::io::Error::other)?;
        let hash = crate::embeddings::pipeline::content_hash(&body);
        if let Some(existing) = db::knowledge::get_diary_by_date(db, date).await? {
            db::knowledge::update_diary(db, &existing.id, &body, Some(&hash)).await?;
        } else {
            db::knowledge::create_diary(db, date, &body, Some(&hash)).await?;
        }
        diary_synced += 1;
    }

    // Embeddings
    if skip_embeddings {
        println!(
            "Reindex complete: notes {created} created / {synced} updated, \
             {ref_synced} references synced, {diary_synced} diary entries synced \
             (embeddings skipped)"
        );
        return Ok(());
    }

    let client = EmbeddingClient::new(embeddings_config);
    if !client.is_available().await {
        eprintln!("Ollama unavailable — skipping embeddings");
        println!(
            "Reindex complete: notes {created} created / {synced} updated, \
             {ref_synced} references synced, {diary_synced} diary entries synced"
        );
        return Ok(());
    }

    db::embeddings::delete_all_embeddings(db).await?;
    let (_discovered, embed_requests) =
        crate::embeddings::pipeline::reconcile_filesystem(db, workspace)
            .await
            .map_err(|e| match e {
                crate::embeddings::pipeline::PipelineError::Embedding(e) => {
                    GhostError::Embedding(e)
                }
                crate::embeddings::pipeline::PipelineError::Database(e) => {
                    GhostError::Database(Box::new(e))
                }
            })?;
    let embedded = crate::embeddings::pipeline::embed_sources(&client, db, embed_requests)
        .await
        .map_err(|e| match e {
            crate::embeddings::pipeline::PipelineError::Embedding(e) => GhostError::Embedding(e),
            crate::embeddings::pipeline::PipelineError::Database(e) => {
                GhostError::Database(Box::new(e))
            }
        })?;

    println!(
        "Reindex complete: notes {created} created / {synced} updated, \
         {ref_synced} references synced, {diary_synced} diary entries synced, \
         {embedded} embeddings generated"
    );
    Ok(())
}

async fn get_note_title(db: &GhostDb, id: &str) -> String {
    db::knowledge::get_note(db, id)
        .await
        .map(|n| n.title)
        .unwrap_or_else(|_| id.to_string())
}
