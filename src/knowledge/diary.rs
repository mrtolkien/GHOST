use std::path::{Path, PathBuf};

use super::error::KnowledgeError;
use super::files::list_md_files;

#[must_use]
pub fn diary_path(workspace: &Path, date: &str) -> PathBuf {
    workspace.join("diary").join(format!("{date}.md"))
}

/// Read today's diary entry, if it exists and is non-empty.
#[must_use]
pub fn load_diary_today(workspace: &Path) -> Option<String> {
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let path = diary_path(workspace, &today);
    match std::fs::read_to_string(&path) {
        Ok(content) if !content.trim().is_empty() => Some(content),
        _ => None,
    }
}

pub fn read_diary(workspace: &Path, date: &str) -> Result<String, KnowledgeError> {
    let path = diary_path(workspace, date);
    std::fs::read_to_string(&path).map_err(|source| KnowledgeError::Io { path, source })
}

pub fn write_diary(workspace: &Path, date: &str, body: &str) -> Result<PathBuf, KnowledgeError> {
    let path = diary_path(workspace, date);

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| KnowledgeError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }

    std::fs::write(&path, body).map_err(|source| KnowledgeError::Io {
        path: path.clone(),
        source,
    })?;

    Ok(path)
}

pub fn list_diary_entries(workspace: &Path) -> Result<Vec<PathBuf>, KnowledgeError> {
    list_md_files(&workspace.join("diary"))
}

/// Load the most recent `count` non-empty diary entries from disk.
/// Returns `(date, body)` pairs in chronological order.
#[must_use]
pub fn load_recent_diary(workspace: &Path, count: usize) -> Vec<(String, String)> {
    let Ok(paths) = list_diary_entries(workspace) else {
        return Vec::new();
    };

    paths
        .iter()
        .rev()
        .filter_map(|path| {
            let date = path.file_stem()?.to_str()?.to_string();
            let body = std::fs::read_to_string(path).ok()?;
            if body.trim().is_empty() {
                return None;
            }
            Some((date, body))
        })
        .take(count)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn diary_write_and_read() {
        let workspace = TempDir::new().unwrap();
        std::fs::create_dir_all(workspace.path().join("diary")).unwrap();

        let path = write_diary(workspace.path(), "2026-02-17", "Today was good.\n").unwrap();
        assert!(path.exists());

        let content = read_diary(workspace.path(), "2026-02-17").unwrap();
        assert_eq!(content, "Today was good.\n");
    }

    #[test]
    fn load_recent_diary_returns_last_n_entries() {
        let workspace = TempDir::new().unwrap();
        let diary_dir = workspace.path().join("diary");
        std::fs::create_dir_all(&diary_dir).unwrap();

        std::fs::write(diary_dir.join("2026-03-05.md"), "Day five.").unwrap();
        std::fs::write(diary_dir.join("2026-03-06.md"), "Day six.").unwrap();
        std::fs::write(diary_dir.join("2026-03-07.md"), "Day seven.").unwrap();
        std::fs::write(diary_dir.join("2026-03-08.md"), "").unwrap(); // empty, skipped

        let entries = load_recent_diary(workspace.path(), 2);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].0, "2026-03-06");
        assert_eq!(entries[0].1, "Day six.");
        assert_eq!(entries[1].0, "2026-03-07");
        assert_eq!(entries[1].1, "Day seven.");
    }

    #[test]
    fn load_recent_diary_empty_dir() {
        let workspace = TempDir::new().unwrap();
        let entries = load_recent_diary(workspace.path(), 2);
        assert!(entries.is_empty());
    }
}
