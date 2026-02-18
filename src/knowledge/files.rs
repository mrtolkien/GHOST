use std::path::{Path, PathBuf};

use super::error::KnowledgeError;
use super::parser::{parse_note, serialize_note};
use super::types::{NoteFrontMatter, ParsedNote};

#[must_use]
pub fn note_path(workspace: &Path, slug: &str) -> PathBuf {
    workspace.join("notes").join(format!("{slug}.md"))
}

#[must_use]
pub fn reference_path(workspace: &Path, topic: &str, filename: &str) -> PathBuf {
    workspace.join("references").join(topic).join(filename)
}

#[must_use]
pub fn diary_path(workspace: &Path, date: &str) -> PathBuf {
    workspace.join("diary").join(format!("{date}.md"))
}

pub fn read_note(workspace: &Path, filename: &str) -> Result<ParsedNote, KnowledgeError> {
    let path = workspace.join("notes").join(filename);
    let content = std::fs::read_to_string(&path).map_err(|source| KnowledgeError::Io {
        path: path.clone(),
        source,
    })?;
    parse_note(&content)
}

pub fn write_note(
    workspace: &Path,
    front: &NoteFrontMatter,
    body: &str,
) -> Result<PathBuf, KnowledgeError> {
    let slug = super::parser::slug_from_title(&front.title);
    let path = note_path(workspace, &slug);

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| KnowledgeError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }

    let content = serialize_note(front, body)?;
    std::fs::write(&path, &content).map_err(|source| KnowledgeError::Io {
        path: path.clone(),
        source,
    })?;

    Ok(path)
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

pub fn list_notes(workspace: &Path) -> Result<Vec<PathBuf>, KnowledgeError> {
    list_md_files(&workspace.join("notes"))
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

pub fn list_diary_entries(workspace: &Path) -> Result<Vec<PathBuf>, KnowledgeError> {
    list_md_files(&workspace.join("diary"))
}

fn list_md_files(dir: &Path) -> Result<Vec<PathBuf>, KnowledgeError> {
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

fn collect_md_files_recursive(
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
    use crate::knowledge::types::Archetype;
    use tempfile::TempDir;

    #[test]
    fn write_then_read_roundtrip() {
        let workspace = TempDir::new().unwrap();
        std::fs::create_dir_all(workspace.path().join("notes")).unwrap();

        let front = NoteFrontMatter {
            title: "Test Note".to_string(),
            archetype: Some(Archetype::Concept),
            tags: vec!["test".into()],
            trust: 7,
        };
        let body = "This is the body.\n";

        let path = write_note(workspace.path(), &front, body).unwrap();
        assert!(path.exists());
        assert!(path.to_str().unwrap().contains("test_note.md"));

        let parsed = read_note(workspace.path(), "test_note.md").unwrap();
        assert_eq!(parsed.front.title, "Test Note");
        assert_eq!(parsed.front.trust, 7);
        assert_eq!(parsed.body, body);
    }

    #[test]
    fn list_notes_finds_files() {
        let workspace = TempDir::new().unwrap();
        let notes_dir = workspace.path().join("notes");
        std::fs::create_dir_all(&notes_dir).unwrap();

        std::fs::write(notes_dir.join("alpha.md"), "content").unwrap();
        std::fs::write(notes_dir.join("beta.md"), "content").unwrap();
        std::fs::write(notes_dir.join("not_a_note.txt"), "content").unwrap();

        let notes = list_notes(workspace.path()).unwrap();
        assert_eq!(notes.len(), 2);
    }

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
    fn diary_write_and_read() {
        let workspace = TempDir::new().unwrap();
        std::fs::create_dir_all(workspace.path().join("diary")).unwrap();

        let path = write_diary(workspace.path(), "2026-02-17", "Today was good.\n").unwrap();
        assert!(path.exists());

        let content = read_diary(workspace.path(), "2026-02-17").unwrap();
        assert_eq!(content, "Today was good.\n");
    }

    #[test]
    fn list_notes_empty_dir() {
        let workspace = TempDir::new().unwrap();
        let notes = list_notes(workspace.path()).unwrap();
        assert!(notes.is_empty());
    }

    #[test]
    fn note_path_construction() {
        let p = note_path(Path::new("/workspace"), "rust");
        assert_eq!(p, PathBuf::from("/workspace/notes/rust.md"));
    }

    #[test]
    fn reference_path_construction() {
        let p = reference_path(Path::new("/workspace"), "ai", "paper.md");
        assert_eq!(p, PathBuf::from("/workspace/references/ai/paper.md"));
    }
}
