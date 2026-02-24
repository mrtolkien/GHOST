use clap::Subcommand;
use surrealdb::sql::Thing;

use crate::db;
use crate::embeddings::EmbeddingClient;
use crate::error::GhostError;
use crate::knowledge;

#[derive(Debug, Subcommand)]
pub enum KnowledgeCommand {
    Search {
        query: String,
        #[arg(long)]
        kind: Option<String>,
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
    let db = crate::db::connect(&config.workspace).await?;

    match command {
        KnowledgeCommand::Search { query, kind, limit } => {
            cmd_search(&db, &config.embeddings, &query, kind.as_deref(), limit).await
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

async fn cmd_search(
    db: &surrealdb::Surreal<surrealdb::engine::local::Db>,
    embeddings_config: &crate::config::EmbeddingsConfig,
    query: &str,
    kind: Option<&str>,
    limit: usize,
) -> Result<(), GhostError> {
    let mut bm25_hits = Vec::new();

    if kind.is_none() || kind == Some("note") {
        bm25_hits.extend(db::knowledge::search_notes(db, query, limit).await?);
    }
    if kind.is_none() || kind == Some("reference") {
        bm25_hits.extend(db::knowledge::search_references(db, query, limit).await?);
    }
    if kind.is_none() || kind == Some("diary") {
        bm25_hits.extend(db::knowledge::search_diary(db, query, limit).await?);
    }

    // Try hybrid search: embed query and merge with BM25
    let client = EmbeddingClient::new(embeddings_config);
    let hits = if client.is_available().await {
        match client.embed_batch(&[query.to_string()]).await {
            Ok(vectors) if !vectors.is_empty() => {
                let embedding_hits = db::embeddings::vector_search(db, &vectors[0], limit).await?;
                db::knowledge::hybrid_merge(&bm25_hits, &embedding_hits, limit)
            }
            Ok(_) => {
                logfire::warn!("embedding returned empty vectors, falling back to BM25");
                fallback_bm25(bm25_hits, limit)
            }
            Err(e) => {
                logfire::warn!(
                    "embedding query failed, falling back to BM25",
                    error = e.to_string()
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
    db: &surrealdb::Surreal<surrealdb::engine::local::Db>,
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
        if let Some(arch) = &note.archetype {
            println!("Archetype: {arch}");
        }
        if !note.tags.is_empty() {
            println!("Tags: {}", note.tags.join(", "));
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
                    if let Some(arch) = &parsed.front.archetype {
                        println!("Archetype: {arch}");
                    }
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

async fn cmd_graph(
    db: &surrealdb::Surreal<surrealdb::engine::local::Db>,
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
                let target_title = get_note_title(db, &edge.out).await;
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
                let source_title = get_note_title(db, &edge.in_node).await;
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

async fn cmd_tags(db: &surrealdb::Surreal<surrealdb::engine::local::Db>) -> Result<(), GhostError> {
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

async fn cmd_recent(
    db: &surrealdb::Surreal<surrealdb::engine::local::Db>,
    limit: usize,
) -> Result<(), GhostError> {
    let items = db::knowledge::list_recent(db, limit).await?;
    if items.is_empty() {
        println!("No knowledge items yet");
        return Ok(());
    }
    for item in &items {
        let date = item.updated_at.to_string();
        let short_date: String = date.chars().take(10).collect();
        println!("{}  [{:>9}]  {}", short_date, item.kind, item.title,);
    }
    Ok(())
}

async fn cmd_stats(
    db: &surrealdb::Surreal<surrealdb::engine::local::Db>,
) -> Result<(), GhostError> {
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

async fn cmd_references(
    db: &surrealdb::Surreal<surrealdb::engine::local::Db>,
    topic: Option<&str>,
    limit: usize,
) -> Result<(), GhostError> {
    let refs = db::knowledge::list_references_by_topic(db, topic, limit).await?;

    if refs.is_empty() {
        match topic {
            Some(t) => println!("No references for topic '{t}'"),
            None => println!("No references"),
        }
        return Ok(());
    }

    let mut current_topic: Option<&str> = None;
    for r in &refs {
        if current_topic != Some(&r.topic) {
            if current_topic.is_some() {
                println!();
            }
            println!("## {}", r.topic);
            current_topic = Some(&r.topic);
        }
        let url = r.source_url.as_deref().unwrap_or("-");
        println!("  {}  ({})", r.path, url);
    }

    Ok(())
}

async fn cmd_reindex(
    db: &surrealdb::Surreal<surrealdb::engine::local::Db>,
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
        let parsed = match knowledge::parse_note(&raw) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("Skipping {}: {e}", path.display());
                continue;
            }
        };

        let archetype_str = parsed.front.archetype.map(|a| a.to_string());
        match db::knowledge::find_note_by_title(db, &parsed.front.title).await? {
            Some(existing) => {
                // Compute relative path from file's position
                let rel_path = path
                    .strip_prefix(workspace)
                    .ok()
                    .and_then(|p| p.to_str())
                    .map(|s| s.to_string());
                db::knowledge::update_note(
                    db,
                    &existing.id,
                    &parsed.body,
                    archetype_str.as_deref(),
                    &parsed.front.tags,
                    &parsed.front.sources,
                    parsed.front.trust,
                    rel_path.as_deref(),
                )
                .await?;
                let _result = knowledge::reconcile::reconcile_edges(
                    db,
                    &existing.id,
                    &parsed.front.title,
                    &parsed.wiki_links,
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
                    .map(|s| s.to_string());
                let note_id = db::knowledge::create_note_full(
                    db,
                    &parsed.front.title,
                    &parsed.body,
                    archetype_str.as_deref(),
                    &parsed.front.tags,
                    &parsed.front.sources,
                    parsed.front.trust,
                    rel_path.as_deref(),
                )
                .await?;
                let _result = knowledge::reconcile::reconcile_edges(
                    db,
                    &note_id,
                    &parsed.front.title,
                    &parsed.wiki_links,
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
        let rel_path = path
            .strip_prefix(workspace)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();
        let topic = path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|f| f.to_str())
            .unwrap_or("unknown");

        if db::knowledge::find_reference_by_path(db, &rel_path)
            .await?
            .is_none()
        {
            let content = std::fs::read_to_string(path).map_err(std::io::Error::other)?;
            db::knowledge::create_reference(db, topic, &rel_path, &content, None).await?;
            ref_synced += 1;
        }
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
        if db::knowledge::get_diary_by_date(db, date).await?.is_none() {
            let body = std::fs::read_to_string(path).map_err(std::io::Error::other)?;
            db::knowledge::create_diary(db, date, &body).await?;
            diary_synced += 1;
        }
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
    let (embedded, _skipped) = crate::embeddings::pipeline::reconcile_embeddings(&client, db)
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

async fn get_note_title(
    db: &surrealdb::Surreal<surrealdb::engine::local::Db>,
    id: &Thing,
) -> String {
    db::knowledge::get_note(db, id)
        .await
        .map(|n| n.title)
        .unwrap_or_else(|_| id.to_string())
}
