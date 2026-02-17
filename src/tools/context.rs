use std::path::{Path, PathBuf};

use surrealdb::Surreal;
use surrealdb::engine::local::Db;

use crate::config::Config;

use super::ToolError;

#[derive(Debug, Clone)]
pub struct ToolContext {
    pub workspace: PathBuf,
    pub cwd: PathBuf,
    pub db: Surreal<Db>,
    pub config: Config,
    pub session_id: String,
}

/// Resolve a path relative to a base directory and enforce that the result
/// stays within the workspace boundary.
///
/// - Absolute paths are used directly (but still checked against workspace).
/// - Relative paths are joined to `base`.
/// - The resolved path is canonicalized to prevent `..` escapes.
///
/// Returns the resolved absolute path or a `PermissionDenied` error.
#[tracing::instrument(skip_all, level = "debug")]
pub fn resolve_path(raw_path: &str, base: &Path, workspace: &Path) -> Result<PathBuf, ToolError> {
    let raw = Path::new(raw_path);

    let joined = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        base.join(raw)
    };

    // Normalize the path without requiring it to exist (for write_file
    // creating new files). We use a simple component-based normalization.
    let normalized = normalize_path(&joined);

    let ws_canonical = workspace
        .canonicalize()
        .unwrap_or_else(|_| workspace.to_path_buf());

    if !normalized.starts_with(&ws_canonical) {
        return Err(ToolError::PermissionDenied(format!(
            "path '{}' is outside the workspace '{}'",
            normalized.display(),
            ws_canonical.display(),
        )));
    }

    Ok(normalized)
}

/// Normalize a path by resolving `.` and `..` components without filesystem
/// access. This allows checking paths for files that don't exist yet.
fn normalize_path(path: &Path) -> PathBuf {
    use std::path::Component;

    // Start from the canonical form of the path's existing ancestors, then
    // append the remainder. This handles symlinks in the existing portion.
    let mut existing = path.to_path_buf();
    let mut suffix_components = Vec::new();

    loop {
        match existing.canonicalize() {
            Ok(canonical) => {
                let mut result = canonical;
                for component in suffix_components.into_iter().rev() {
                    result.push(component);
                }
                return result;
            }
            Err(_) => {
                if let Some(file_name) = existing.file_name() {
                    suffix_components.push(file_name.to_owned());
                    existing.pop();
                } else {
                    break;
                }
            }
        }
    }

    // Fallback: purely logical normalization (no existing ancestors found)
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                if !components.is_empty() {
                    components.pop();
                }
            }
            Component::CurDir => {}
            other => components.push(other),
        }
    }
    components.iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn resolve_relative_path_within_workspace() {
        let workspace = TempDir::new().unwrap();
        let result = resolve_path("subdir/file.txt", workspace.path(), workspace.path());
        assert!(result.is_ok());
        let resolved = result.unwrap();
        assert!(resolved.starts_with(workspace.path().canonicalize().unwrap()));
    }

    #[test]
    fn resolve_absolute_path_within_workspace() {
        let workspace = TempDir::new().unwrap();
        let abs = workspace.path().join("notes/test.md");
        let result = resolve_path(abs.to_str().unwrap(), workspace.path(), workspace.path());
        assert!(result.is_ok());
    }

    #[test]
    fn reject_path_outside_workspace() {
        let workspace = TempDir::new().unwrap();
        let result = resolve_path("../../etc/passwd", workspace.path(), workspace.path());
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, ToolError::PermissionDenied(_)));
    }

    #[test]
    fn reject_absolute_path_outside_workspace() {
        let workspace = TempDir::new().unwrap();
        let result = resolve_path("/etc/passwd", workspace.path(), workspace.path());
        assert!(result.is_err());
    }
}
