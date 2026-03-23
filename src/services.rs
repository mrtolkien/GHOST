use std::path::Path;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use toml_edit::{DocumentMut, Item, Table, value};

/// A single service entry from services.toml.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceEntry {
    pub start: Option<String>,
    pub stop: Option<String>,
    pub update: Option<String>,
    pub status: Option<String>,
}

/// Ordered collection of service entries.
/// IndexMap preserves TOML insertion order (required for top-to-bottom execution).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ServiceRegistry {
    pub entries: IndexMap<String, ServiceEntry>,
}

impl ServiceRegistry {
    /// Load from a services.toml file. Returns error if file is missing or malformed.
    ///
    /// Uses `toml_edit` internally to preserve the insertion order of table entries,
    /// which is required for correct top-to-bottom start/stop sequencing.
    pub fn load(path: &Path) -> Result<Self, ServiceRegistryError> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| ServiceRegistryError::Io(path.to_path_buf(), e))?;
        Self::parse(&content).map_err(|e| ServiceRegistryError::Parse(path.to_path_buf(), e))
    }

    /// Load from file, returning an empty registry if the file doesn't exist.
    pub fn load_or_empty(path: &Path) -> Result<Self, ServiceRegistryError> {
        if !path.exists() {
            return Ok(Self::default());
        }
        Self::load(path)
    }

    /// Parse TOML content into a `ServiceRegistry`, preserving insertion order.
    ///
    /// Uses `toml_edit` to iterate tables in document order, then deserialises
    /// each individual table with the standard `toml` crate so we can reuse the
    /// derived `Deserialize` impl on `ServiceEntry`.
    fn parse(content: &str) -> Result<Self, String> {
        let doc: toml_edit::DocumentMut = content
            .parse()
            .map_err(|e: toml_edit::TomlError| e.to_string())?;

        let mut entries = IndexMap::new();
        for (key, item) in doc.iter() {
            let table = item
                .as_table()
                .ok_or_else(|| format!("'{key}' is not a table"))?;

            // Serialise the individual table back to TOML text, then use the
            // derived Deserialize impl on ServiceEntry.
            let entry_toml = table.to_string();
            let entry: ServiceEntry =
                toml::from_str(&entry_toml).map_err(|e| format!("{key}: {e}"))?;

            entries.insert(key.to_string(), entry);
        }

        Ok(Self { entries })
    }

    /// Entry names in file order.
    pub fn names(&self) -> Vec<&str> {
        self.entries.keys().map(|s| s.as_str()).collect()
    }

    /// Add a new service entry. Errors if name already exists.
    pub fn add(&mut self, name: String, entry: ServiceEntry) -> Result<(), ServiceRegistryError> {
        if entry.start.is_none()
            && entry.stop.is_none()
            && entry.update.is_none()
            && entry.status.is_none()
        {
            return Err(ServiceRegistryError::EmptyEntry);
        }
        if self.entries.contains_key(&name) {
            return Err(ServiceRegistryError::AlreadyExists(name));
        }
        self.entries.insert(name, entry);
        Ok(())
    }

    /// Remove a service entry by name. Errors if not found.
    pub fn remove(&mut self, name: &str) -> Result<(), ServiceRegistryError> {
        if self.entries.shift_remove(name).is_none() {
            return Err(ServiceRegistryError::NotFound(name.to_string()));
        }
        Ok(())
    }

    /// Write the registry back to a TOML file.
    ///
    /// Uses `toml_edit` to produce top-level `[name]` tables (matching the load
    /// format), rather than nested `[entries.name]` tables that `toml::to_string_pretty`
    /// would emit given the `entries` field name.
    pub fn save(&self, path: &Path) -> Result<(), ServiceRegistryError> {
        let mut doc = DocumentMut::new();
        for (name, entry) in &self.entries {
            let mut table = Table::new();
            if let Some(ref cmd) = entry.start {
                table.insert("start", value(cmd.as_str()));
            }
            if let Some(ref cmd) = entry.stop {
                table.insert("stop", value(cmd.as_str()));
            }
            if let Some(ref cmd) = entry.update {
                table.insert("update", value(cmd.as_str()));
            }
            if let Some(ref cmd) = entry.status {
                table.insert("status", value(cmd.as_str()));
            }
            doc.insert(name, Item::Table(table));
        }
        std::fs::write(path, doc.to_string())
            .map_err(|e| ServiceRegistryError::Io(path.to_path_buf(), e))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ServiceRegistryError {
    #[error("cannot read {0}: {1}")]
    Io(std::path::PathBuf, std::io::Error),
    #[error("invalid TOML in {0}: {1}")]
    Parse(std::path::PathBuf, String),
    #[error("service '{0}' already exists")]
    AlreadyExists(String),
    #[error("service '{0}' not found")]
    NotFound(String),
    #[error("at least one command field is required")]
    EmptyEntry,
    #[error("cannot serialize services: {0}")]
    Serialize(#[from] toml::ser::Error),
    #[error("{service}: command failed (exit {code})\n{stderr}")]
    CommandFailed {
        service: String,
        code: i32,
        stderr: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_toml(content: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f
    }

    #[test]
    fn parse_full_entry() {
        let f = write_toml(
            r#"
[containers]
start = "podman compose up -d"
stop = "podman compose down"
update = "podman compose pull"
status = "podman compose ps"
"#,
        );
        let reg = ServiceRegistry::load(f.path()).unwrap();
        assert_eq!(reg.names(), vec!["containers"]);
        let e = &reg.entries["containers"];
        assert_eq!(e.start.as_deref(), Some("podman compose up -d"));
        assert_eq!(e.stop.as_deref(), Some("podman compose down"));
    }

    #[test]
    fn parse_partial_entry() {
        let f = write_toml(
            r#"
[docling]
start = "systemctl --user start docling-serve"
stop = "systemctl --user stop docling-serve"
"#,
        );
        let reg = ServiceRegistry::load(f.path()).unwrap();
        let e = &reg.entries["docling"];
        assert!(e.update.is_none());
        assert!(e.status.is_none());
    }

    #[test]
    fn parse_preserves_order() {
        let f = write_toml(
            r#"
[containers]
start = "a"

[llama-server]
start = "b"

[docling]
start = "c"
"#,
        );
        let reg = ServiceRegistry::load(f.path()).unwrap();
        assert_eq!(reg.names(), vec!["containers", "llama-server", "docling"]);
    }

    #[test]
    fn load_missing_file_errors() {
        let result = ServiceRegistry::load(Path::new("/nonexistent/services.toml"));
        assert!(result.is_err());
    }

    #[test]
    fn load_or_empty_missing_file() {
        let reg = ServiceRegistry::load_or_empty(Path::new("/nonexistent/services.toml")).unwrap();
        assert!(reg.entries.is_empty());
    }

    #[test]
    fn parse_malformed_toml() {
        let f = write_toml("not valid [[ toml");
        assert!(ServiceRegistry::load(f.path()).is_err());
    }

    #[test]
    fn add_and_remove() {
        let mut reg = ServiceRegistry::default();
        reg.add(
            "foo".into(),
            ServiceEntry {
                start: Some("start-foo".into()),
                stop: None,
                update: None,
                status: None,
            },
        )
        .unwrap();
        assert_eq!(reg.names(), vec!["foo"]);
        reg.remove("foo").unwrap();
        assert!(reg.entries.is_empty());
    }

    #[test]
    fn add_duplicate_errors() {
        let mut reg = ServiceRegistry::default();
        reg.add(
            "foo".into(),
            ServiceEntry {
                start: Some("x".into()),
                stop: None,
                update: None,
                status: None,
            },
        )
        .unwrap();
        assert!(reg
            .add(
                "foo".into(),
                ServiceEntry {
                    start: Some("y".into()),
                    stop: None,
                    update: None,
                    status: None,
                },
            )
            .is_err());
    }

    #[test]
    fn remove_missing_errors() {
        let mut reg = ServiceRegistry::default();
        assert!(reg.remove("nope").is_err());
    }

    #[test]
    fn add_empty_entry_errors() {
        let mut reg = ServiceRegistry::default();
        assert!(reg
            .add(
                "foo".into(),
                ServiceEntry {
                    start: None,
                    stop: None,
                    update: None,
                    status: None,
                },
            )
            .is_err());
    }

    #[test]
    fn save_roundtrip() {
        let mut reg = ServiceRegistry::default();
        reg.add(
            "svc".into(),
            ServiceEntry {
                start: Some("start-cmd".into()),
                stop: Some("stop-cmd".into()),
                update: None,
                status: None,
            },
        )
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("services.toml");
        reg.save(&path).unwrap();
        let loaded = ServiceRegistry::load(&path).unwrap();
        assert_eq!(loaded.entries["svc"].start.as_deref(), Some("start-cmd"));
    }
}
