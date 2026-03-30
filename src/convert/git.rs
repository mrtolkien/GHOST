use std::path::{Path, PathBuf};

use tokio::process::Command;

use crate::reference_import::ImportError;

use super::staging::{create_staging_dir, slug_from_source};

/// Result of converting a git repository into a staging directory.
#[derive(Debug)]
pub struct GitConvertResult {
    /// Path to the staging directory containing the checked-out files.
    pub staging_dir: PathBuf,
    /// The commit hash (from `git rev-parse HEAD`) of the cloned repo.
    pub version_ref: String,
    /// The original repository URL.
    pub source_url: String,
}

/// Clone a git repository and write matching files to a staging directory.
///
/// Performs a shallow blobless clone for efficiency, optionally sparse-checking
/// out only the requested `paths`, filtering by `extensions`, and copying the
/// result into a new staging directory under `staging_root`.
#[tracing::instrument(
    name = "convert_git",
    skip_all,
    fields(url = %url, staging_root = %staging_root.display())
)]
pub async fn convert_git(
    url: &str,
    paths: &[String],
    extensions: &[String],
    git_ref: Option<&str>,
    staging_root: &Path,
) -> Result<GitConvertResult, ImportError> {
    let tmp_dir = tempfile::tempdir()?;
    let repo_dir = tmp_dir.path().join("repo");

    // Phase 1: shallow blobless clone
    clone_repo(url, git_ref, &repo_dir).await?;

    // Phase 2: sparse checkout if paths specified
    if !paths.is_empty() {
        sparse_checkout(&repo_dir, paths).await?;
    }
    run_git(&repo_dir, &["checkout"]).await?;

    // Get commit hash
    let version_ref = run_git_output(&repo_dir, &["rev-parse", "HEAD"]).await?;
    let version_ref = version_ref.trim().to_string();

    // Walk files, filtering by paths and extensions
    let files = walk_files(&repo_dir, paths, extensions);

    if files.is_empty() {
        return Err(ImportError::Git(format!(
            "no files matched filters in {url}"
        )));
    }

    // Create staging directory and copy matching files into it
    let slug = slug_from_source(url);
    let staging_dir = create_staging_dir(staging_root, &slug)?;

    if let Err(e) = copy_files_to_staging(&repo_dir, &files, &staging_dir) {
        // Clean up staging dir on failure
        let _ = std::fs::remove_dir_all(&staging_dir);
        return Err(e);
    }

    Ok(GitConvertResult {
        staging_dir,
        version_ref,
        source_url: url.to_string(),
    })
}

/// Run `git clone` with shallow blobless options.
async fn clone_repo(
    url: &str,
    git_ref: Option<&str>,
    repo_dir: &Path,
) -> Result<(), ImportError> {
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

    let output = Command::new("git")
        .args(&clone_args)
        .arg(repo_dir)
        .output()
        .await
        .map_err(|e| ImportError::Git(format!("failed to spawn git: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ImportError::Git(format!("git clone failed: {stderr}")));
    }

    Ok(())
}

/// Set up sparse checkout for the specified paths.
async fn sparse_checkout(
    repo_dir: &Path,
    paths: &[String],
) -> Result<(), ImportError> {
    run_git(repo_dir, &["sparse-checkout", "init", "--cone"]).await?;
    let mut args = vec!["sparse-checkout", "set"];
    let path_strs: Vec<&str> = paths.iter().map(String::as_str).collect();
    args.extend(path_strs);
    run_git(repo_dir, &args).await
}

/// Copy files from the repo into the staging directory, preserving relative paths.
fn copy_files_to_staging(
    repo_dir: &Path,
    files: &[PathBuf],
    staging_dir: &Path,
) -> Result<(), ImportError> {
    for file_path in files {
        let rel_path = file_path
            .strip_prefix(repo_dir)
            .unwrap_or(file_path);
        let dest = staging_dir.join(rel_path);

        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Skip binary/unreadable files — only copy valid UTF-8 text
        let Ok(content) = std::fs::read_to_string(file_path) else {
            continue;
        };
        std::fs::write(&dest, content)?;
    }

    Ok(())
}

/// Walk the repo directory, filtering by paths and extensions.
fn walk_files(
    repo_dir: &Path,
    paths: &[String],
    extensions: &[String],
) -> Vec<PathBuf> {
    let mut results = Vec::new();

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

/// Recursively collect files from `dir`, skipping `.git` and filtering by extension.
fn walk_dir_recursive(
    dir: &Path,
    extensions: &[String],
    out: &mut Vec<PathBuf>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == ".git") {
                continue;
            }
            walk_dir_recursive(&path, extensions, out);
        } else if path.is_file() && matches_extensions(&path, extensions) {
            out.push(path);
        }
    }
}

/// Check whether a file's extension matches the allowed list.
///
/// Extensions are compared case-insensitively and must include the leading dot
/// (e.g. `.md`, `.rs`). An empty list means all extensions match.
fn matches_extensions(path: &Path, extensions: &[String]) -> bool {
    if extensions.is_empty() {
        return true;
    }
    let ext = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()));
    match ext {
        Some(e) => extensions.iter().any(|allowed| allowed.eq_ignore_ascii_case(&e)),
        None => false,
    }
}

/// Run a git command in the repo directory, returning an error on failure.
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

/// Run a git command and capture its stdout as a string.
async fn run_git_output(
    repo_dir: &Path,
    args: &[&str],
) -> Result<String, ImportError> {
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
