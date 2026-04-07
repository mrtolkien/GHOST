use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::db;
use crate::db::GhostDb;
use crate::knowledge;

use super::types::{ImportConfigJson, ImportError};

const IMPORT_TOML_FILENAME: &str = "_import.toml";

/// Load an `ImportConfigJson` from the DB's `import_batch.import_config`
/// column. Returns `None` if no batch exists or if `import_config` is null.
pub async fn load_import_config_from_db(
    db: &GhostDb,
    topic_id: &str,
) -> Result<Option<ImportConfigJson>, ImportError> {
    let batch = db::knowledge::get_import_batch_by_topic(db, topic_id).await?;
    match batch.and_then(|b| b.import_config) {
        Some(json) => {
            let config: ImportConfigJson = serde_json::from_str(&json)
                .map_err(|e| ImportError::Config(format!("invalid import_config JSON: {e}")))?;
            Ok(Some(config))
        }
        None => Ok(None),
    }
}

/// Ensure that a topic hierarchy exists in the DB. For "dioxus/docs",
/// ensures both "dioxus" and "dioxus/docs" exist as topic rows.
/// Returns the leaf topic ID.
pub async fn ensure_topic_hierarchy(db: &GhostDb, topic_name: &str) -> Result<String, ImportError> {
    let parts: Vec<&str> = topic_name.split('/').collect();
    let mut last_id = String::new();

    for i in 0..parts.len() {
        let name = parts[..=i].join("/");
        last_id = db::knowledge::find_or_create_topic(db, &name).await?;
    }

    Ok(last_id)
}

/// Flat structure serialized into `_import.toml` — merges `ImportConfigJson`
/// fields with runtime values (version_ref, ref_count).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ImportToml {
    source_type: String,
    source_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    version_ref: Option<String>,
    #[serde(default)]
    ref_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    git_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    extensions: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    max_depth: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    max_pages: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    no_ocr: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    page_range: Option<(u32, u32)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    authors: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    publisher: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    publication_date: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RepairImportMetadata {
    pub config: ImportConfigJson,
    pub version_ref: Option<String>,
}

impl ImportToml {
    fn into_config_json(self) -> ImportConfigJson {
        ImportConfigJson {
            source_type: self.source_type,
            source_url: self.source_url,
            git_ref: self.git_ref,
            paths: self.paths,
            extensions: self.extensions,
            max_depth: self.max_depth,
            max_pages: self.max_pages,
            no_ocr: self.no_ocr,
            page_range: self.page_range,
            title: self.title,
            authors: self.authors,
            language: self.language,
            publisher: self.publisher,
            publication_date: self.publication_date,
        }
    }
}

/// Read an `ImportConfigJson` from the `_import.toml` file on disk for the
/// given topic. Extra fields (version_ref, ref_count) are ignored by the
/// returned config but preserved by the repair validation helper.
pub fn read_import_toml(
    workspace: &Path,
    topic_name: &str,
) -> Result<ImportConfigJson, ImportError> {
    Ok(read_import_toml_file(workspace, topic_name)?.into_config_json())
}

/// Validate that `_import.toml` contains the provenance required to
/// reconstruct import state from workspace files during repair-sensitive paths.
pub fn validate_import_metadata_for_repair(
    workspace: &Path,
    topic_name: &str,
) -> Result<RepairImportMetadata, ImportError> {
    let import_toml = read_import_toml_file(workspace, topic_name)?;
    validate_import_toml_for_repair(&import_toml, topic_name, true)?;

    Ok(RepairImportMetadata {
        version_ref: import_toml.version_ref.clone(),
        config: import_toml.into_config_json(),
    })
}

/// Load update-critical import metadata from disk, backfilling `_import.toml`
/// from the DB for older supported topics when possible.
pub async fn ensure_update_metadata(
    db: &GhostDb,
    workspace: &Path,
    topic_id: &str,
    topic_name: &str,
) -> Result<RepairImportMetadata, ImportError> {
    match load_update_metadata_from_disk(workspace, topic_name) {
        Ok(metadata) => Ok(metadata),
        Err(original_error) => {
            backfill_update_metadata_from_db(db, workspace, topic_id, topic_name, original_error)
                .await
        }
    }
}

fn read_import_toml_file(workspace: &Path, topic_name: &str) -> Result<ImportToml, ImportError> {
    let path = workspace
        .join("references")
        .join(topic_name)
        .join(IMPORT_TOML_FILENAME);
    let content = std::fs::read_to_string(&path).map_err(|e| {
        ImportError::Config(format!("no _import.toml for topic '{topic_name}': {e}"))
    })?;
    toml::from_str(&content).map_err(|e| {
        ImportError::Config(format!(
            "invalid _import.toml for topic '{topic_name}': {e}"
        ))
    })
}

fn validate_import_toml_for_repair(
    import_toml: &ImportToml,
    topic_name: &str,
    require_version_ref: bool,
) -> Result<(), ImportError> {
    if import_toml.source_type.trim().is_empty() || import_toml.source_url.trim().is_empty() {
        return Err(repair_metadata_error(
            topic_name,
            &import_toml.source_type,
            "source_type and source_url",
        ));
    }

    match import_toml.source_type.as_str() {
        "git" => {
            if require_version_ref && import_toml.version_ref.is_none() {
                return Err(repair_metadata_error(topic_name, "git", "version_ref"));
            }
        }
        "crawl" => {
            if import_toml.max_depth.is_none() || import_toml.max_pages.is_none() {
                return Err(repair_metadata_error(
                    topic_name,
                    "crawl",
                    "max_depth and max_pages",
                ));
            }
        }
        "file" | "book" | "page" => {}
        other => {
            return Err(ImportError::Config(format!(
                "topic '{topic_name}' has unsupported source_type '{other}' in _import.toml for repair reconstruction"
            )));
        }
    }

    Ok(())
}

fn load_update_metadata_from_disk(
    workspace: &Path,
    topic_name: &str,
) -> Result<RepairImportMetadata, ImportError> {
    let import_toml = read_import_toml_file(workspace, topic_name)?;
    validate_import_toml_for_update(&import_toml, topic_name)?;
    Ok(RepairImportMetadata {
        version_ref: import_toml.version_ref.clone(),
        config: import_toml.into_config_json(),
    })
}

async fn backfill_update_metadata_from_db(
    db: &GhostDb,
    workspace: &Path,
    topic_id: &str,
    topic_name: &str,
    original_error: ImportError,
) -> Result<RepairImportMetadata, ImportError> {
    let Some(import_toml) = load_import_toml_from_db(db, topic_id).await? else {
        return Err(original_error);
    };
    validate_import_toml_for_update(&import_toml, topic_name)?;
    write_import_toml(
        workspace,
        topic_name,
        &import_toml.clone().into_config_json(),
        import_toml.version_ref.as_deref(),
        import_toml.ref_count,
    )?;
    Ok(RepairImportMetadata {
        version_ref: import_toml.version_ref.clone(),
        config: import_toml.into_config_json(),
    })
}

fn validate_import_toml_for_update(
    import_toml: &ImportToml,
    topic_name: &str,
) -> Result<(), ImportError> {
    if import_toml.source_type.trim().is_empty() || import_toml.source_url.trim().is_empty() {
        return Err(repair_metadata_error(
            topic_name,
            &import_toml.source_type,
            "source_type and source_url",
        ));
    }

    match import_toml.source_type.as_str() {
        "git" | "crawl" => validate_import_toml_for_repair(import_toml, topic_name, false),
        other => Err(ImportError::Config(format!(
            "unsupported source_type for update: {other}"
        ))),
    }
}

fn repair_metadata_error(topic_name: &str, source_type: &str, missing: &str) -> ImportError {
    let source_label = if source_type.is_empty() {
        "unknown"
    } else {
        source_type
    };
    ImportError::Config(format!(
        "topic '{topic_name}' is missing repair-critical import metadata in _import.toml for source_type '{source_label}': required {missing}"
    ))
}

async fn load_import_toml_from_db(
    db: &GhostDb,
    topic_id: &str,
) -> Result<Option<ImportToml>, ImportError> {
    let Some(batch) = db::knowledge::get_import_batch_by_topic(db, topic_id).await? else {
        return Ok(None);
    };
    let Some(import_config) = batch.import_config.as_deref() else {
        return Ok(None);
    };
    let config: ImportConfigJson = serde_json::from_str(import_config)
        .map_err(|e| ImportError::Config(format!("invalid import_config JSON: {e}")))?;
    Ok(Some(ImportToml {
        source_type: config.source_type,
        source_url: config.source_url,
        version_ref: batch.version_ref,
        ref_count: usize::try_from(batch.ref_count.max(0)).unwrap_or_default(),
        git_ref: config.git_ref,
        paths: config.paths,
        extensions: config.extensions,
        max_depth: config.max_depth,
        max_pages: config.max_pages,
        no_ocr: config.no_ocr,
        page_range: config.page_range,
        title: config.title,
        authors: config.authors,
        language: config.language,
        publisher: config.publisher,
        publication_date: config.publication_date,
    }))
}

/// Write `_import.toml` alongside imported references to record import metadata.
pub fn write_import_toml(
    workspace: &Path,
    topic_name: &str,
    config: &ImportConfigJson,
    version_ref: Option<&str>,
    ref_count: usize,
) -> Result<(), ImportError> {
    let ref_dir = workspace.join("references").join(topic_name);
    std::fs::create_dir_all(&ref_dir)?;

    let import_toml = ImportToml {
        source_type: config.source_type.clone(),
        source_url: config.source_url.clone(),
        version_ref: version_ref.map(String::from),
        ref_count,
        git_ref: config.git_ref.clone(),
        paths: config.paths.clone(),
        extensions: config.extensions.clone(),
        max_depth: config.max_depth,
        max_pages: config.max_pages,
        no_ocr: config.no_ocr,
        page_range: config.page_range,
        title: config.title.clone(),
        authors: config.authors.clone(),
        language: config.language.clone(),
        publisher: config.publisher.clone(),
        publication_date: config.publication_date.clone(),
    };

    let content = format!(
        "# Auto-generated by ghost reference import\n{}",
        toml::to_string_pretty(&import_toml)
            .map_err(|e| ImportError::Io(std::io::Error::other(e.to_string())))?
    );

    let import_path = ref_dir.join(IMPORT_TOML_FILENAME);
    std::fs::write(&import_path, content)?;

    // Also ensure index notes exist for the topic hierarchy
    knowledge::ensure_index_notes(workspace, topic_name)
        .map_err(|e| ImportError::Io(std::io::Error::other(e.to_string())))?;

    Ok(())
}
