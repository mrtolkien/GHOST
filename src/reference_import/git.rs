use std::path::{Path, PathBuf};

use tokio::process::Command;

use crate::db;
use crate::db::GhostDb;

use super::topic::ensure_topic_hierarchy;
use super::types::{ImportConfig, ImportConfigJson, ImportError, ImportResult, ImportSource};

/// Clone a git repo and return a manifest of (topic-relative path, content) pairs
/// without touching the database.
pub(crate) async fn fetch_git_manifest(
    config: &ImportConfig,
) -> Result<(String, Vec<(String, String)>), ImportError> {
    let ImportSource::Git {
        url,
        paths,
        extensions,
        git_ref,
    } = &config.source
    else {
        return Err(ImportError::Git("expected git source".into()));
    };

    let tmp_dir = tempfile::tempdir()?;
    let repo_dir = tmp_dir.path().join("repo");

    // Phase 1: shallow blobless clone
    let mut clone_args = vec![
        "clone",
        "--no-checkout",
        "--depth",
        "1",
        "--filter=blob:none",
    ];
    if let Some(r) = git_ref {
        clone_args.push("--branch");
        clone_args.push(r);
    }
    clone_args.push(url);

    let status = Command::new("git")
        .args(&clone_args)
        .arg(&repo_dir)
        .output()
        .await
        .map_err(|e| ImportError::Git(format!("failed to spawn git: {e}")))?;

    if !status.status.success() {
        let stderr = String::from_utf8_lossy(&status.stderr);
        return Err(ImportError::Git(format!("git clone failed: {stderr}")));
    }

    // Phase 2: sparse checkout if paths specified
    if !paths.is_empty() {
        run_git(&repo_dir, &["sparse-checkout", "init", "--cone"]).await?;
        let mut args = vec!["sparse-checkout", "set"];
        let path_strs: Vec<&str> = paths.iter().map(String::as_str).collect();
        args.extend(path_strs);
        run_git(&repo_dir, &args).await?;
    }
    run_git(&repo_dir, &["checkout"]).await?;

    // Get commit hash
    let commit_hash = run_git_output(&repo_dir, &["rev-parse", "HEAD"]).await?;
    let commit_hash = commit_hash.trim().to_string();

    // Walk files and collect as (topic-relative path, content)
    let files = walk_files(&repo_dir, paths, extensions);
    let mut manifest = Vec::new();
    for file_path in &files {
        let rel_path = file_path
            .strip_prefix(&repo_dir)
            .unwrap_or(file_path)
            .to_string_lossy();
        let ref_path = format!("{}/{}", config.topic, rel_path);

        let Ok(content) = std::fs::read_to_string(file_path) else {
            continue; // skip binary/unreadable
        };

        manifest.push((ref_path, content));
    }

    Ok((commit_hash, manifest))
}

/// Import references from a git repository using sparse checkout.
///
/// Two-phase clone for large repos:
/// 1. `git clone --no-checkout --depth 1 --filter=blob:none`
/// 2. sparse-checkout + selective checkout
#[tracing::instrument(name = "import git", skip_all, fields(topic = %config.topic))]
pub async fn import_git(
    db: &GhostDb,
    workspace: &Path,
    config: &ImportConfig,
) -> Result<ImportResult, ImportError> {
    let ImportSource::Git { url, .. } = &config.source else {
        return Err(ImportError::Git("expected git source".into()));
    };
    let url = url.clone();

    let (version_ref, manifest) = fetch_git_manifest(config).await?;

    // Ensure topic hierarchy in DB
    let topic_id = ensure_topic_hierarchy(db, &config.topic).await?;

    // Build serializable config snapshot for DB and TOML
    let config_json = ImportConfigJson::from(config);
    let config_json_str = serde_json::to_string(&config_json).ok();

    let total_files = manifest.len();
    println!("Found {total_files} files to process");
    let mut created = 0usize;
    let mut skipped = 0usize;

    // Upsert import batch (we'll update ref_count after creating references)
    let batch_id = db::knowledge::upsert_import_batch(
        db,
        &topic_id,
        "git",
        &url,
        Some(&version_ref),
        0, // placeholder, updated below
        config_json_str.as_deref(),
    )
    .await?;

    for (ref_path, content) in &manifest {
        // Idempotency: skip if reference with this path already exists
        if db::knowledge::find_reference_by_path(db, ref_path)
            .await?
            .is_some()
        {
            skipped += 1;
            continue;
        }

        let processed = created + skipped + 1;
        println!("  [{processed}/{total_files}] {ref_path}");

        // Write to disk: references/{ref_path}
        let disk_path = workspace.join("references").join(ref_path);
        if let Some(parent) = disk_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&disk_path, content)?;

        let hash = crate::embeddings::pipeline::content_hash(content);
        db::knowledge::create_reference(
            db,
            &topic_id,
            ref_path,
            content,
            Some(&url),
            Some(&batch_id),
            Some(&hash),
        )
        .await?;

        created += 1;
    }

    // Update import batch with final ref count
    let total_refs = usize::try_from(
        db::knowledge::count_references_by_topic(db, &topic_id)
            .await?
            .max(0),
    )
    .unwrap_or(0);
    let batch_id = db::knowledge::upsert_import_batch(
        db,
        &topic_id,
        "git",
        &url,
        Some(&version_ref),
        total_refs as i64,
        config_json_str.as_deref(),
    )
    .await?;

    // Write _import.toml and ensure index notes
    super::topic::write_import_toml(
        workspace,
        &config.topic,
        &config_json,
        Some(&version_ref),
        total_refs,
    )?;

    Ok(ImportResult {
        topic_id,
        batch_id,
        references_created: created,
        references_skipped: skipped,
    })
}

/// Walk the repo directory, filtering by paths and extensions.
fn walk_files(repo_dir: &Path, paths: &[String], extensions: &[String]) -> Vec<PathBuf> {
    let mut results = Vec::new();

    // Determine roots to walk
    let roots: Vec<PathBuf> = if paths.is_empty() {
        vec![repo_dir.to_path_buf()]
    } else {
        paths.iter().map(|p| repo_dir.join(p)).collect()
    };

    for root in &roots {
        if !root.exists() {
            continue;
        }
        walk_dir_recursive(root, extensions, &mut results);
    }

    results
}

fn walk_dir_recursive(dir: &Path, extensions: &[String], out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // Skip .git directory
            if path.file_name().is_some_and(|n| n == ".git") {
                continue;
            }
            walk_dir_recursive(&path, extensions, out);
        } else if path.is_file() && matches_extensions(&path, extensions) {
            out.push(path);
        }
    }
}

fn matches_extensions(path: &Path, extensions: &[String]) -> bool {
    if extensions.is_empty() {
        return true;
    }
    let ext = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()));
    match ext {
        Some(e) => extensions.contains(&e),
        None => false,
    }
}

async fn run_git(repo_dir: &Path, args: &[&str]) -> Result<(), ImportError> {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo_dir)
        .output()
        .await
        .map_err(|e| ImportError::Git(format!("failed to spawn git: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ImportError::Git(format!(
            "git {} failed: {stderr}",
            args.first().unwrap_or(&"")
        )));
    }
    Ok(())
}

async fn run_git_output(repo_dir: &Path, args: &[&str]) -> Result<String, ImportError> {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo_dir)
        .output()
        .await
        .map_err(|e| ImportError::Git(format!("failed to spawn git: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ImportError::Git(format!(
            "git {} failed: {stderr}",
            args.first().unwrap_or(&"")
        )));
    }

    String::from_utf8(output.stdout)
        .map_err(|e| ImportError::Git(format!("invalid UTF-8 output: {e}")))
}
