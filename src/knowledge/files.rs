use std::path::{Path, PathBuf};

use super::error::KnowledgeError;

#[must_use]
pub fn reference_path(workspace: &Path, topic: &str, filename: &str) -> PathBuf {
    workspace.join("references").join(topic).join(filename)
}

pub fn list_references(workspace: &Path) -> Result<Vec<PathBuf>, KnowledgeError> {
    let base = workspace.join("references");
    if !base.exists() {
        return Ok(Vec::new());
    }
    let mut results = Vec::new();
    collect_md_files_recursive(&base, &mut results)?;
    results.sort();
    Ok(results)
}

pub(super) fn list_md_files(dir: &Path) -> Result<Vec<PathBuf>, KnowledgeError> {
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|source| KnowledgeError::Io {
            path: dir.to_path_buf(),
            source,
        })?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("md") {
                Some(path)
            } else {
                None
            }
        })
        .collect();

    entries.sort();
    Ok(entries)
}

pub(super) fn collect_md_files_recursive(
    dir: &Path,
    results: &mut Vec<PathBuf>,
) -> Result<(), KnowledgeError> {
    let entries = std::fs::read_dir(dir).map_err(|source| KnowledgeError::Io {
        path: dir.to_path_buf(),
        source,
    })?;

    for entry in entries {
        let entry = entry.map_err(|source| KnowledgeError::Io {
            path: dir.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == ".archive") {
                continue;
            }
            collect_md_files_recursive(&path, results)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
            results.push(path);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn list_references_recursive() {
        let workspace = TempDir::new().unwrap();
        let refs_dir = workspace.path().join("references/topic_a");
        std::fs::create_dir_all(&refs_dir).unwrap();

        std::fs::write(refs_dir.join("ref1.md"), "content").unwrap();

        let refs = list_references(workspace.path()).unwrap();
        assert_eq!(refs.len(), 1);
    }

    #[test]
    fn reference_path_construction() {
        let p = reference_path(Path::new("/workspace"), "ai", "paper.md");
        assert_eq!(p, PathBuf::from("/workspace/references/ai/paper.md"));
    }
}
