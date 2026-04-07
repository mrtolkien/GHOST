pub use crate::db::repair_types::{
    RepairReport, TablePolicy, TableVerification, backup_path, candidate_db_path, report_path,
};

use std::path::Path;

use tempfile::TempDir;

use crate::config;
use crate::db;
use crate::embeddings::EmbeddingClient;
use crate::error::GhostError;
use crate::reference_import::validate_import_metadata_for_repair;

pub async fn execute(dry_run: bool) -> Result<(), GhostError> {
    let _observability = crate::observability::init()?;
    let config = config::load()?;
    let report = Box::pin(repair_database_with_embeddings(
        Path::new(&config.workspace),
        Some(&config.embeddings),
        dry_run,
    ))
    .await?;

    if report.success {
        if dry_run {
            println!(
                "Repair dry-run succeeded. Candidate DB written to {}",
                report.candidate_db.display()
            );
        } else {
            println!(
                "Repair succeeded. Live DB replaced at {}",
                report.live_db.display()
            );
        }
    } else {
        println!(
            "Repair failed closed. Candidate DB left at {}",
            report.candidate_db.display()
        );
    }
    println!(
        "Repair report: {}",
        report_path(&report.live_db, &report.timestamp).display()
    );
    Ok(())
}

pub async fn repair_database(
    workspace: &Path,
    embedding_dim: usize,
    dry_run: bool,
) -> Result<RepairReport, GhostError> {
    let embeddings_config = config::EmbeddingsConfig {
        url: String::new(),
        model: String::new(),
        batch_size: 0,
        dimension: embedding_dim,
    };
    Box::pin(repair_database_with_embeddings(
        workspace,
        Some(&embeddings_config),
        dry_run,
    ))
    .await
}

async fn repair_database_with_embeddings(
    workspace: &Path,
    embeddings_config: Option<&config::EmbeddingsConfig>,
    dry_run: bool,
) -> Result<RepairReport, GhostError> {
    let live_db = workspace.join("ghost.db");
    if !live_db.exists() {
        return Err(GhostError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("database not found at {}", live_db.display()),
        )));
    }

    let stamp = chrono::Utc::now().to_rfc3339().replace(':', "-");
    let candidate_db = candidate_db_path(&live_db, &stamp);
    let report_path = report_path(&live_db, &stamp);
    let candidate_workspace = tempfile::Builder::new()
        .prefix("ghost-db-repair-")
        .tempdir()
        .map_err(GhostError::Io)?;
    let candidate_workspace_db = candidate_workspace.path().join("ghost.db");

    let embedding_dim = embeddings_config
        .map(|config| config.dimension)
        .unwrap_or_default();
    let repair_result = Box::pin(build_candidate_database(
        workspace,
        &candidate_workspace,
        embeddings_config,
        embedding_dim,
        &live_db,
    ))
    .await;

    let mut report = match repair_result {
        Ok(tables) => RepairReport {
            timestamp: stamp.clone(),
            candidate_db: candidate_db.clone(),
            live_db: live_db.clone(),
            backup_db: None,
            tables,
            success: true,
            failure_reason: None,
        },
        Err(error) => RepairReport {
            timestamp: stamp.clone(),
            candidate_db: candidate_db.clone(),
            live_db: live_db.clone(),
            backup_db: None,
            tables: Vec::new(),
            success: false,
            failure_reason: Some(error.to_string()),
        },
    };

    std::fs::copy(&candidate_workspace_db, &candidate_db).map_err(GhostError::Io)?;

    if report.success && !dry_run {
        let backup_db = backup_path(&live_db, &stamp);
        match swap_live_database(&live_db, &candidate_db, &backup_db) {
            Ok(()) => {
                report.backup_db = Some(backup_db);
                report.candidate_db = live_db.clone();
            }
            Err(error) => {
                report.success = false;
                report.failure_reason = Some(error.to_string());
            }
        }
    }

    write_report(&report_path, &report)?;
    Ok(report)
}

async fn build_candidate_database(
    workspace: &Path,
    candidate_workspace: &TempDir,
    embeddings_config: Option<&config::EmbeddingsConfig>,
    embedding_dim: usize,
    live_db: &Path,
) -> Result<Vec<TableVerification>, GhostError> {
    let candidate_db = db::connect(candidate_workspace.path(), embedding_dim)
        .await
        .map_err(database_error)?;
    Box::pin(rebuild_file_backed_state(
        &candidate_db,
        workspace,
        embeddings_config,
    ))
    .await?;
    rebuild_import_batches_from_disk(&candidate_db, workspace).await?;
    candidate_db.close().await;

    let candidate_workspace_db = candidate_workspace.path().join("ghost.db");
    crate::db::repair_copy::copy_db_only_tables(&candidate_workspace_db, live_db)
        .await
        .map_err(database_error)?;
    crate::db::repair_verify::verify_db_only_tables(&candidate_workspace_db, live_db)
        .await
        .map_err(database_error)
}

async fn rebuild_file_backed_state(
    db: &db::GhostDb,
    workspace: &Path,
    embeddings_config: Option<&config::EmbeddingsConfig>,
) -> Result<(), GhostError> {
    let (_, embed_requests) = Box::pin(crate::embeddings::pipeline::reconcile_filesystem(
        db, workspace,
    ))
    .await
    .map_err(pipeline_error)?;
    if let Some(config) = embeddings_config {
        rebuild_embeddings(db, config, embed_requests).await?;
    }
    Ok(())
}

async fn rebuild_import_batches_from_disk(
    candidate_db: &db::GhostDb,
    workspace: &Path,
) -> Result<(), GhostError> {
    for topic_name in topics_with_import_toml(workspace)? {
        let metadata = validate_import_metadata_for_repair(workspace, &topic_name)
            .map_err(GhostError::Import)?;
        let topic = db::knowledge::find_topic_by_name(candidate_db, &topic_name)
            .await
            .map_err(database_error)?
            .ok_or_else(|| {
                GhostError::Other(format!(
                    "repair rebuilt references for topic '{topic_name}' but no topic row exists"
                ))
            })?;
        let ref_count = db::knowledge::count_references_by_topic(candidate_db, &topic.id)
            .await
            .map_err(database_error)?;
        let import_config = serde_json::to_string(&metadata.config)
            .map_err(|error| GhostError::Other(format!("serialize import config: {error}")))?;
        db::knowledge::upsert_import_batch(
            candidate_db,
            &topic.id,
            &metadata.config.source_type,
            &metadata.config.source_url,
            metadata.version_ref.as_deref(),
            ref_count,
            Some(&import_config),
        )
        .await
        .map_err(database_error)?;
    }
    Ok(())
}

fn topics_with_import_toml(workspace: &Path) -> Result<Vec<String>, GhostError> {
    let references_root = workspace.join("references");
    if !references_root.exists() {
        return Ok(Vec::new());
    }
    let mut topics = Vec::new();
    collect_import_topics(&references_root, &references_root, &mut topics)?;
    topics.sort();
    topics.dedup();
    Ok(topics)
}

fn collect_import_topics(
    root: &Path,
    dir: &Path,
    topics: &mut Vec<String>,
) -> Result<(), GhostError> {
    for entry in std::fs::read_dir(dir).map_err(GhostError::Io)? {
        let entry = entry.map_err(GhostError::Io)?;
        let path = entry.path();
        if path.is_dir() {
            collect_import_topics(root, &path, topics)?;
            continue;
        }
        if path.file_name().and_then(|name| name.to_str()) != Some("_import.toml") {
            continue;
        }
        let topic = path
            .parent()
            .and_then(|parent| parent.strip_prefix(root).ok())
            .and_then(|relative| relative.to_str())
            .ok_or_else(|| {
                GhostError::Other(format!(
                    "failed to derive topic from import metadata path {}",
                    path.display()
                ))
            })?;
        topics.push(topic.to_string());
    }
    Ok(())
}

fn write_report(path: &Path, report: &RepairReport) -> Result<(), GhostError> {
    let content = serde_json::to_string_pretty(report)
        .map_err(|error| GhostError::Other(format!("serialize repair report: {error}")))?;
    std::fs::write(path, content).map_err(GhostError::Io)
}

fn database_error(error: crate::db::DatabaseError) -> GhostError {
    GhostError::Database(Box::new(error))
}

fn pipeline_error(error: crate::embeddings::pipeline::PipelineError) -> GhostError {
    match error {
        crate::embeddings::pipeline::PipelineError::Embedding(error) => {
            GhostError::Embedding(error)
        }
        crate::embeddings::pipeline::PipelineError::Database(error) => database_error(error),
    }
}

async fn rebuild_embeddings(
    db: &db::GhostDb,
    config: &config::EmbeddingsConfig,
    embed_requests: Vec<crate::embeddings::pipeline::EmbedRequest>,
) -> Result<(), GhostError> {
    if embed_requests.is_empty() {
        return Ok(());
    }
    if config.url.is_empty() || config.model.is_empty() || config.batch_size == 0 {
        return Ok(());
    }

    let client = EmbeddingClient::new(config);
    if !client.is_available().await {
        return Err(GhostError::Other(
            "embeddings service unavailable during db repair".to_string(),
        ));
    }
    crate::embeddings::pipeline::embed_sources(&client, db, embed_requests)
        .await
        .map(|_| ())
        .map_err(pipeline_error)
}

fn swap_live_database(
    live_db: &Path,
    candidate_db: &Path,
    backup_db: &Path,
) -> Result<(), GhostError> {
    std::fs::rename(live_db, backup_db).map_err(GhostError::Io)?;
    if let Err(error) = std::fs::rename(candidate_db, live_db) {
        let rollback_error = std::fs::rename(backup_db, live_db).err();
        let message = match rollback_error {
            Some(rollback_error) => format!(
                "failed to install repaired database: {error}; rollback also failed: {rollback_error}"
            ),
            None => {
                format!("failed to install repaired database: {error}; restored original live DB")
            }
        };
        return Err(GhostError::Other(message));
    }
    Ok(())
}
