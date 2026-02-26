use std::path::{Path, PathBuf};

use super::error::KnowledgeError;
use super::parser::{parse_note, serialize_note};
use super::types::{NoteFrontMatter, ParsedNote};

#[must_use]
pub fn note_path(workspace: &Path, subfolder: Option<&str>, slug: &str) -> PathBuf {
    let mut base = workspace.join("notes");
    if let Some(sub) = subfolder {
        base = base.join(sub);
    }
    base.join(format!("{slug}.md"))
}

/// Extract the subfolder path from the first tag (if any).
/// The first tag is treated as a hierarchical path (e.g. `3d-printing/hardware`).
#[must_use]
pub fn subfolder_from_tags(tags: &[String]) -> Option<&str> {
    tags.first().map(String::as_str)
}

#[must_use]
pub fn reference_path(workspace: &Path, topic: &str, filename: &str) -> PathBuf {
    workspace.join("references").join(topic).join(filename)
}

#[must_use]
pub fn diary_path(workspace: &Path, date: &str) -> PathBuf {
    workspace.join("diary").join(format!("{date}.md"))
}

/// Read a note by relative path (e.g. `test_note.md` or
/// `3d-printing/hardware/bambu_p2s.md`).
pub fn read_note(workspace: &Path, relative_path: &str) -> Result<ParsedNote, KnowledgeError> {
    let path = workspace.join("notes").join(relative_path);
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
    let subfolder = subfolder_from_tags(&front.tags);
    let path = note_path(workspace, subfolder, &slug);

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

/// Compute the relative path of a note within the workspace
/// (e.g. `notes/3d-printing/hardware/bambu_p2s.md`).
#[must_use]
pub fn note_relative_path(subfolder: Option<&str>, slug: &str) -> String {
    match subfolder {
        Some(sub) => format!("notes/{sub}/{slug}.md"),
        None => format!("notes/{slug}.md"),
    }
}

/// Ensure index (topic hub) notes exist at each level of a subfolder path.
///
/// For example, subfolder `3d-printing/hardware` creates:
/// - `notes/3d-printing/index.md`
/// - `notes/3d-printing/hardware/index.md`
///
/// Idempotent — skips levels that already have an index file.
pub fn ensure_index_notes(
    workspace: &Path,
    subfolder: &str,
) -> Result<Vec<PathBuf>, KnowledgeError> {
    let mut created = Vec::new();
    let parts: Vec<&str> = subfolder.split('/').collect();

    for i in 0..parts.len() {
        let folder_path: String = parts[..=i].join("/");
        let index_path = workspace.join("notes").join(&folder_path).join("index.md");

        if index_path.exists() {
            continue;
        }

        if let Some(parent) = index_path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| KnowledgeError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }

        let title = parts[i]
            .replace('-', " ")
            .split_whitespace()
            .map(|w| {
                let mut c = w.chars();
                match c.next() {
                    None => String::new(),
                    Some(f) => f.to_uppercase().to_string() + c.as_str(),
                }
            })
            .collect::<Vec<_>>()
            .join(" ");

        let front = NoteFrontMatter {
            title,
            archetype: Some(super::types::Archetype::Topic),
            tags: vec![folder_path.clone()],
            sources: vec![],
            trust: 5,
        };
        let body = format!("Knowledge hub for {}.\n", front.title);
        let content = serialize_note(&front, &body)?;
        std::fs::write(&index_path, &content).map_err(|source| KnowledgeError::Io {
            path: index_path.clone(),
            source,
        })?;

        created.push(index_path);
    }

    Ok(created)
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
    let base = workspace.join("notes");
    if !base.exists() {
        return Ok(Vec::new());
    }
    let mut results = Vec::new();
    collect_md_files_recursive(&base, &mut results)?;
    results.sort();
    Ok(results)
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
            sources: vec![],
            trust: 7,
        };
        let body = "This is the body.\n";

        let path = write_note(workspace.path(), &front, body).unwrap();
        assert!(path.exists());
        assert!(path.to_str().unwrap().contains("test_note.md"));

        // With tag "test", note is placed under notes/test/
        let parsed = read_note(workspace.path(), "test/test_note.md").unwrap();
        assert_eq!(parsed.front.title, "Test Note");
        assert_eq!(parsed.front.trust, 7);
        assert_eq!(parsed.body, body);
    }

    #[test]
    fn write_note_no_tags_goes_flat() {
        let workspace = TempDir::new().unwrap();

        let front = NoteFrontMatter {
            title: "Flat Note".to_string(),
            archetype: None,
            tags: vec![],
            sources: vec![],
            trust: 5,
        };

        let path = write_note(workspace.path(), &front, "body\n").unwrap();
        assert!(path.ends_with("notes/flat_note.md"));
    }

    #[test]
    fn ensure_index_notes_creates_hierarchy() {
        let workspace = TempDir::new().unwrap();
        let created = ensure_index_notes(workspace.path(), "3d-printing/hardware").unwrap();
        assert_eq!(created.len(), 2);
        assert!(workspace.path().join("notes/3d-printing/index.md").exists());
        assert!(
            workspace
                .path()
                .join("notes/3d-printing/hardware/index.md")
                .exists()
        );

        // Calling again should create nothing (idempotent)
        let again = ensure_index_notes(workspace.path(), "3d-printing/hardware").unwrap();
        assert!(again.is_empty());
    }

    #[test]
    fn list_notes_finds_files_recursively() {
        let workspace = TempDir::new().unwrap();
        let notes_dir = workspace.path().join("notes");
        let sub_dir = notes_dir.join("topic");
        std::fs::create_dir_all(&sub_dir).unwrap();

        std::fs::write(notes_dir.join("alpha.md"), "content").unwrap();
        std::fs::write(sub_dir.join("beta.md"), "content").unwrap();
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
        let p = note_path(Path::new("/workspace"), None, "rust");
        assert_eq!(p, PathBuf::from("/workspace/notes/rust.md"));
    }

    #[test]
    fn note_path_with_subfolder() {
        let p = note_path(
            Path::new("/workspace"),
            Some("3d-printing/hardware"),
            "bambu_p2s",
        );
        assert_eq!(
            p,
            PathBuf::from("/workspace/notes/3d-printing/hardware/bambu_p2s.md")
        );
    }

    #[test]
    fn subfolder_from_tags_extracts_first() {
        let tags = vec!["3d-printing/hardware".to_string(), "review".to_string()];
        assert_eq!(subfolder_from_tags(&tags), Some("3d-printing/hardware"));
        assert_eq!(subfolder_from_tags(&[]), None);
    }

    #[test]
    fn reference_path_construction() {
        let p = reference_path(Path::new("/workspace"), "ai", "paper.md");
        assert_eq!(p, PathBuf::from("/workspace/references/ai/paper.md"));
    }
}
