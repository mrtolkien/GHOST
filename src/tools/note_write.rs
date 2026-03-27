use std::str::FromStr;

use super::output::ToolOutput;
use async_trait::async_trait;
use serde_json::{Value, json};

use crate::db;
use crate::db::knowledge::NoteInput;
use crate::knowledge::reconcile::reconcile_edges;
use crate::knowledge::{self, Archetype, NoteFrontMatter, extract_wiki_links};
use crate::providers::ToolDefinition;
use crate::web::url_match::{extract_frontmatter_info, urls_match};

use super::context::ToolContext;
use super::error::ToolError;
use super::manager::Tool;

pub struct NoteWrite;

#[async_trait]
impl Tool for NoteWrite {
    fn name(&self) -> &str {
        "note_write"
    }

    fn schema(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Create or update a knowledge note. \
                Notes are stored as markdown files with TOML frontmatter \
                and indexed in the knowledge graph."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["create", "update", "archive"],
                        "description": "Whether to create a new note, update an existing one, or archive it"
                    },
                    "title": {
                        "type": "string",
                        "description": "Note title — short Wikipedia-style noun phrase (used as filename slug and graph node name). No prefixes or parentheticals."
                    },
                    "body": {
                        "type": "string",
                        "description": "Markdown body content. May contain [[WikiLinks]] and [[rel>Target]] typed links."
                    },
                    "tags": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Optional tags for categorization"
                    },
                    "sources": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Source URLs for attribution. Each URL is preserved in frontmatter. Use this instead of putting bare URLs in the body."
                    },
                    "trust": {
                        "type": "integer",
                        "description": "Trust level 1-10 (defaults based on archetype)"
                    },
                    "archetype": {
                        "type": "string",
                        "enum": ["entity", "analysis", "source", "profile", "topic"],
                        "description": "Note archetype. entity=factual description, analysis=reasoning framework, source=source evaluation, profile=OPERATOR info, topic=navigation hub."
                    },
                    "parent": {
                        "type": "string",
                        "description": "Title of parent note for hierarchy (e.g. 'Nvidia' for 'RTX 4090')."
                    }
                },
                "required": ["action", "title"],
                "additionalProperties": false
            }),
        }
    }

    #[tracing::instrument(skip_all, fields(tool = "note_write"))]
    async fn execute(&self, params: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let action = params
            .get("action")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ToolError::InvalidParams("missing required parameter 'action'".into())
            })?;
        let title = params
            .get("title")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::InvalidParams("missing required parameter 'title'".into()))?;

        // Archive only needs title
        if action == "archive" {
            return self.archive_note(ctx, title).await.map(ToolOutput::text);
        }

        // Create and update need body, archetype, and the rest
        let body = params
            .get("body")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::InvalidParams("missing 'body' for create/update".into()))?;
        let archetype = params
            .get("archetype")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ToolError::InvalidParams("missing 'archetype' for create/update".into())
            })?;
        let archetype = Archetype::from_str(archetype).map_err(ToolError::InvalidParams)?;
        let tags: Vec<String> = params
            .get("tags")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_str)
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default();
        let sources: Vec<String> = params
            .get("sources")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_str)
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default();
        let parent = params
            .get("parent")
            .and_then(Value::as_str)
            .map(String::from);
        let trust = params
            .get("trust")
            .and_then(Value::as_i64)
            .unwrap_or_else(|| archetype.default_trust());

        let archetype_str = archetype.to_string();
        let note = NoteInput {
            title,
            body,
            tags: &tags,
            sources: &sources,
            trust,
            archetype: Some(&archetype_str),
            ..Default::default()
        };

        match action {
            "create" => {
                self.create_note(ctx, &note, archetype, parent.as_deref())
                    .await
            }
            "update" => {
                self.update_note(ctx, &note, archetype, parent.as_deref())
                    .await
            }
            _ => Err(ToolError::InvalidParams(format!(
                "unknown action '{action}', expected 'create', 'update', or 'archive'"
            ))),
        }
        .map(ToolOutput::text)
    }
}

impl NoteWrite {
    /// Check if source URLs are backed by fetched web cache or existing references.
    ///
    /// For each `https://` URL in sources:
    /// - Check `.cache/{session_id}/*.md` files for matching url: frontmatter
    /// - Check references DB via `find_reference_by_url`
    ///
    /// Returns (verified_urls, warnings) where warnings list URLs found only
    /// in old refs.
    async fn verify_source_urls(
        workspace: &std::path::Path,
        session_id: &str,
        db: &crate::db::GhostDb,
        sources: &[String],
    ) -> Result<Vec<String>, ToolError> {
        let https_urls: Vec<&str> = sources
            .iter()
            .map(|s| s.as_str())
            .filter(|s| s.starts_with("https://"))
            .collect();

        if https_urls.is_empty() {
            return Ok(Vec::new());
        }

        // Read all cache files for this session once
        let cache_dir = workspace.join(format!(".cache/{session_id}"));
        let mut cache_urls: Vec<String> = Vec::new();
        if cache_dir.is_dir() {
            let entries = std::fs::read_dir(&cache_dir).map_err(|e| {
                ToolError::ExecutionFailed(format!("failed to read cache dir: {e}"))
            })?;
            for entry in entries {
                let entry = entry.map_err(|e| {
                    ToolError::ExecutionFailed(format!("failed to read cache entry: {e}"))
                })?;
                let path = entry.path();
                if path.extension().is_some_and(|ext| ext == "md")
                    && let Ok(content) = std::fs::read_to_string(&path)
                {
                    let (url, _is_search) = extract_frontmatter_info(&content);
                    if !url.is_empty() {
                        cache_urls.push(url);
                    }
                }
            }
        }

        let mut warnings = Vec::new();
        for url in &https_urls {
            let in_cache = cache_urls.iter().any(|cached| urls_match(cached, url));
            if in_cache {
                continue;
            }
            // Not in cache — check references DB
            let in_refs = db::knowledge::find_reference_by_url(db, url)
                .await
                .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
            if in_refs.is_some() {
                warnings.push(format!(
                    "Source URL {url} found in old references but \
                     not in current session cache — the page was \
                     not freshly fetched this session."
                ));
            } else {
                return Err(ToolError::ExecutionFailed(format!(
                    "Source URL {url} not found in web cache or \
                     references. Fetch the page with web_fetch \
                     before citing it."
                )));
            }
        }

        Ok(warnings)
    }

    /// Strip `[[references/...]]` wiki links that point to non-existent files.
    ///
    /// Regular wiki links (e.g. `[[Bambu Lab]]`) are fine — those create stubs.
    /// But `[[references/topic/filename]]` pointing to a non-existent file would
    /// be a broken citation. We strip these and return a warning message.
    fn sanitize_reference_links(
        workspace: &std::path::Path,
        body: &str,
    ) -> (String, Option<String>) {
        let links = extract_wiki_links(body);
        let missing: Vec<&str> = links
            .iter()
            .filter(|link| link.target.starts_with("references/"))
            .filter(|link| {
                let path = workspace.join(&link.target);
                let path_md = workspace.join(format!("{}.md", link.target));
                !path.exists() && !path_md.exists()
            })
            .map(|link| link.target.as_str())
            .collect();

        if missing.is_empty() {
            return (body.to_string(), None);
        }

        // Strip the broken reference links from the body
        let mut sanitized = body.to_string();
        for target in &missing {
            // Remove [[references/...]] patterns — try with relationship prefix too
            let plain = format!("[[{target}]]");
            sanitized = sanitized.replace(&plain, "");
            // Also handle [[rel>references/...]] patterns
            for prefix in &["source>", "from>", "cited_in>"] {
                let with_rel = format!("[[{prefix}{target}]]");
                sanitized = sanitized.replace(&with_rel, "");
            }
        }

        let warning = format!(
            "Stripped {} broken reference link(s) — the referenced file(s) do not exist. \
             Review the URL or slug.\n\
             Removed:\n{}",
            missing.len(),
            missing
                .iter()
                .map(|p| format!("  - [[{p}]]"))
                .collect::<Vec<_>>()
                .join("\n")
        );

        (sanitized, Some(warning))
    }

    /// Create a new knowledge note: write the markdown file, insert a DB record,
    /// reconcile wiki-link edges, and generate index notes for subfolder hierarchy.
    async fn create_note(
        &self,
        ctx: &ToolContext,
        note: &NoteInput<'_>,
        archetype: Archetype,
        parent: Option<&str>,
    ) -> Result<String, ToolError> {
        let source_warnings =
            Self::verify_source_urls(&ctx.workspace, &ctx.session_id, &ctx.db, note.sources)
                .await?;

        let (sanitized_body, ref_warning) =
            Self::sanitize_reference_links(&ctx.workspace, note.body);

        let front = NoteFrontMatter {
            title: note.title.to_string(),
            archetype,
            tags: note.tags.to_vec(),
            parent: parent.map(String::from),
            sources: note.sources.to_vec(),
            trust: note.trust,
            written_at: chrono::Utc::now().to_rfc3339(),
            updated_at: None,
        };

        let subfolder = knowledge::subfolder_from_tags(note.tags);
        let slug = knowledge::slug_from_title(note.title);
        let rel_path = knowledge::note_relative_path(subfolder, &slug);

        let path = knowledge::write_note(&ctx.workspace, &front, &sanitized_body)
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        // Ensure index notes exist for each level of the subfolder
        let mut index_info = String::new();
        if let Some(sub) = subfolder {
            let created = knowledge::ensure_index_notes(&ctx.workspace, sub)
                .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
            if !created.is_empty() {
                let paths: Vec<String> = created.iter().map(|p| p.display().to_string()).collect();
                index_info = format!(
                    "\n\nWARNING: {} skeleton index note(s) were auto-created:\n  {}\n\
                     These contain only a placeholder description. You MUST update them \
                     with a meaningful description of the topic — semantic search relies \
                     on this to discover the topic.",
                    created.len(),
                    paths.join("\n  "),
                );
            }
        }

        let db_note = NoteInput {
            body: &sanitized_body,
            path: Some(&rel_path),
            ..*note
        };
        let note_id = db::knowledge::create_note_full(&ctx.db, &db_note)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        let wiki_links = extract_wiki_links(&sanitized_body);
        let result = reconcile_edges(&ctx.db, &note_id, note.title, &wiki_links, parent)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        let mut msg = format!(
            "Created note '{}' at {}\n\
             DB record: {}\n\
             Edges: {} created, {} stubs created{index_info}",
            note.title,
            path.display(),
            note_id,
            result.created,
            result.stubs_created,
        );
        if let Some(warning) = ref_warning {
            msg.push_str(&format!("\n\n{warning}"));
        }
        if wiki_links.is_empty() {
            msg.push_str(
                "\n\nHINT: This note has no [[wiki links]]. Consider adding links \
                 to related entities to build the knowledge graph.",
            );
        }
        if !result.stub_titles.is_empty() {
            let stubs = result
                .stub_titles
                .iter()
                .map(|t| format!("  - [[{t}]]"))
                .collect::<Vec<_>>()
                .join("\n");
            msg.push_str(&format!(
                "\n\nNew stub notes created from wiki links:\n{stubs}\n\
                 If any of these deserve a full note, create them before your handoff."
            ));
        }
        match archetype {
            Archetype::Analysis => {
                let has_compare = wiki_links.iter().any(|l| {
                    l.relationship
                        .as_deref()
                        .is_some_and(|r| r == "compares" || r == "based_on")
                });
                if !has_compare {
                    msg.push_str(
                        "\n\nWARNING: Analysis note has no \
                         [[compares>...]] or [[based_on>...]] links.",
                    );
                }
            }
            Archetype::Source => {
                let tagged_sources = note.tags.iter().any(|t| t.contains("/sources"));
                if !tagged_sources {
                    msg.push_str(
                        "\n\nWARNING: Source note should be tagged \
                         under */sources.",
                    );
                }
            }
            Archetype::Profile => {
                let tagged_operator = note.tags.iter().any(|t| t.starts_with("operator"));
                if !tagged_operator {
                    msg.push_str(
                        "\n\nWARNING: Profile note should be tagged \
                         under operator/*.",
                    );
                }
            }
            _ => {}
        }
        for warning in &source_warnings {
            msg.push_str(&format!("\n\nWARNING: {warning}"));
        }
        Ok(msg)
    }

    /// Update an existing knowledge note: rewrite the file (moving it if tags
    /// changed the subfolder), update the DB record, and reconcile edges.
    async fn update_note(
        &self,
        ctx: &ToolContext,
        note: &NoteInput<'_>,
        archetype: Archetype,
        parent: Option<&str>,
    ) -> Result<String, ToolError> {
        let source_warnings =
            Self::verify_source_urls(&ctx.workspace, &ctx.session_id, &ctx.db, note.sources)
                .await?;

        let (sanitized_body, ref_warning) =
            Self::sanitize_reference_links(&ctx.workspace, note.body);

        let existing = db::knowledge::find_note_by_title(&ctx.db, note.title)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?
            .ok_or_else(|| {
                ToolError::ExecutionFailed(format!("note '{}' not found", note.title))
            })?;

        // Preserve original written_at from the existing file
        let existing_note = existing
            .path
            .as_deref()
            .and_then(|p| knowledge::read_note(&ctx.workspace, p).ok());
        let written_at = existing_note
            .as_ref()
            .map(|n| n.front.written_at.clone())
            .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());
        let updated_at = Some(chrono::Utc::now().to_rfc3339());

        let subfolder = knowledge::subfolder_from_tags(note.tags);
        let slug = knowledge::slug_from_title(note.title);
        let rel_path = knowledge::note_relative_path(subfolder, &slug);

        // If the note moved to a different path, remove the old file
        if let Some(old_path) = &existing.path
            && *old_path != rel_path
        {
            let old_abs = ctx.workspace.join(old_path);
            if old_abs.exists() {
                let _ = std::fs::remove_file(&old_abs);
            }
        }

        let db_note = NoteInput {
            body: &sanitized_body,
            path: Some(&rel_path),
            ..*note
        };
        db::knowledge::update_note(&ctx.db, &existing.id, &db_note)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        let front = NoteFrontMatter {
            title: note.title.to_string(),
            archetype,
            tags: note.tags.to_vec(),
            parent: parent.map(String::from),
            sources: note.sources.to_vec(),
            trust: note.trust,
            written_at,
            updated_at,
        };
        let path = knowledge::write_note(&ctx.workspace, &front, &sanitized_body)
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        // Ensure index notes exist for each level of the subfolder
        let mut index_info = String::new();
        if let Some(sub) = subfolder {
            let created = knowledge::ensure_index_notes(&ctx.workspace, sub)
                .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
            if !created.is_empty() {
                let paths: Vec<String> = created.iter().map(|p| p.display().to_string()).collect();
                index_info = format!(
                    "\n\nWARNING: {} skeleton index note(s) were auto-created:\n  {}\n\
                     These contain only a placeholder description. You MUST update them \
                     with a meaningful description of the topic — semantic search relies \
                     on this to discover the topic.",
                    created.len(),
                    paths.join("\n  "),
                );
            }
        }

        let wiki_links = extract_wiki_links(&sanitized_body);
        let result = reconcile_edges(&ctx.db, &existing.id, note.title, &wiki_links, parent)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        let mut msg = format!(
            "Updated note '{}' at {}\n\
             Edges: {} created, {} deleted, {} stubs created{index_info}",
            note.title,
            path.display(),
            result.created,
            result.deleted,
            result.stubs_created,
        );
        if let Some(warning) = ref_warning {
            msg.push_str(&format!("\n\n{warning}"));
        }
        if wiki_links.is_empty() {
            msg.push_str(
                "\n\nHINT: This note has no [[wiki links]]. Consider adding links \
                 to related entities to build the knowledge graph.",
            );
        }
        if !result.stub_titles.is_empty() {
            let stubs = result
                .stub_titles
                .iter()
                .map(|t| format!("  - [[{t}]]"))
                .collect::<Vec<_>>()
                .join("\n");
            msg.push_str(&format!(
                "\n\nNew stub notes created from wiki links:\n{stubs}\n\
                 If any of these deserve a full note, create them before your handoff."
            ));
        }
        match archetype {
            Archetype::Analysis => {
                let has_compare = wiki_links.iter().any(|l| {
                    l.relationship
                        .as_deref()
                        .is_some_and(|r| r == "compares" || r == "based_on")
                });
                if !has_compare {
                    msg.push_str(
                        "\n\nWARNING: Analysis note has no \
                         [[compares>...]] or [[based_on>...]] links.",
                    );
                }
            }
            Archetype::Source => {
                let tagged_sources = note.tags.iter().any(|t| t.contains("/sources"));
                if !tagged_sources {
                    msg.push_str(
                        "\n\nWARNING: Source note should be tagged \
                         under */sources.",
                    );
                }
            }
            Archetype::Profile => {
                let tagged_operator = note.tags.iter().any(|t| t.starts_with("operator"));
                if !tagged_operator {
                    msg.push_str(
                        "\n\nWARNING: Profile note should be tagged \
                         under operator/*.",
                    );
                }
            }
            _ => {}
        }
        for warning in &source_warnings {
            msg.push_str(&format!("\n\nWARNING: {warning}"));
        }
        Ok(msg)
    }

    /// Archive a note: move its file to `.archive/`, delete embeddings, and
    /// remove the DB record (CASCADE deletes edges).
    async fn archive_note(&self, ctx: &ToolContext, title: &str) -> Result<String, ToolError> {
        let existing = db::knowledge::find_note_by_title(&ctx.db, title)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?
            .ok_or_else(|| ToolError::ExecutionFailed(format!("note '{title}' not found")))?;

        // Move file to .archive/
        if let Some(rel_path) = &existing.path {
            let src = ctx.workspace.join(rel_path);
            let archive_path = rel_path.replacen("notes/", "notes/.archive/", 1);
            let dest = ctx.workspace.join(&archive_path);
            if src.exists() {
                if let Some(parent) = dest.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
                }
                std::fs::rename(&src, &dest)
                    .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
            }
        }

        // Delete embeddings (no FK CASCADE)
        let _ = crate::db::embeddings::delete_embeddings_for_source(&ctx.db, &existing.id).await;

        // Delete note record (CASCADE deletes relates_to + cited edges)
        db::knowledge::delete_note(&ctx.db, &existing.id)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        Ok(format!(
            "Archived note '{title}'. File moved to .archive/, DB record removed."
        ))
    }
}
