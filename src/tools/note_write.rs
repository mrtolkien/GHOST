use async_trait::async_trait;
use serde_json::{Value, json};

use crate::db;
use crate::knowledge::reconcile::reconcile_edges;
use crate::knowledge::{self, NoteFrontMatter, extract_wiki_links};
use crate::providers::ToolDefinition;

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
                        "enum": ["create", "update"],
                        "description": "Whether to create a new note or update an existing one"
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
                        "description": "Trust level 1-10 (default 5)"
                    }
                },
                "required": ["action", "title", "body"],
                "additionalProperties": false
            }),
        }
    }

    #[tracing::instrument(skip_all, fields(tool = "note_write"))]
    async fn execute(&self, params: Value, ctx: &ToolContext) -> Result<String, ToolError> {
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
        let body = params
            .get("body")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::InvalidParams("missing required parameter 'body'".into()))?;
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
        let trust = params.get("trust").and_then(Value::as_i64).unwrap_or(5);

        match action {
            "create" => {
                self.create_note(ctx, title, body, &tags, &sources, trust)
                    .await
            }
            "update" => {
                self.update_note(ctx, title, body, &tags, &sources, trust)
                    .await
            }
            _ => Err(ToolError::InvalidParams(format!(
                "unknown action '{action}', expected 'create' or 'update'"
            ))),
        }
    }
}

impl NoteWrite {
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
    #[allow(clippy::too_many_arguments)]
    async fn create_note(
        &self,
        ctx: &ToolContext,
        title: &str,
        body: &str,
        tags: &[String],
        sources: &[String],
        trust: i64,
    ) -> Result<String, ToolError> {
        let (sanitized_body, ref_warning) = Self::sanitize_reference_links(&ctx.workspace, body);

        let front = NoteFrontMatter {
            title: title.to_string(),
            tags: tags.to_vec(),
            sources: sources.to_vec(),
            trust,
        };

        let subfolder = knowledge::subfolder_from_tags(tags);
        let slug = knowledge::slug_from_title(title);
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

        let note_id = db::knowledge::create_note_full(
            &ctx.db,
            title,
            &sanitized_body,
            tags,
            sources,
            trust,
            None,
            Some(&rel_path),
        )
        .await
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        let wiki_links = extract_wiki_links(&sanitized_body);
        let result = reconcile_edges(&ctx.db, &note_id, title, &wiki_links)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        let mut msg = format!(
            "Created note '{}' at {}\n\
             DB record: {}\n\
             Edges: {} created, {} stubs created{index_info}",
            title,
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
        Ok(msg)
    }

    /// Update an existing knowledge note: rewrite the file (moving it if tags
    /// changed the subfolder), update the DB record, and reconcile edges.
    #[allow(clippy::too_many_arguments)]
    async fn update_note(
        &self,
        ctx: &ToolContext,
        title: &str,
        body: &str,
        tags: &[String],
        sources: &[String],
        trust: i64,
    ) -> Result<String, ToolError> {
        let (sanitized_body, ref_warning) = Self::sanitize_reference_links(&ctx.workspace, body);

        let existing = db::knowledge::find_note_by_title(&ctx.db, title)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?
            .ok_or_else(|| ToolError::ExecutionFailed(format!("note '{title}' not found")))?;

        let subfolder = knowledge::subfolder_from_tags(tags);
        let slug = knowledge::slug_from_title(title);
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

        db::knowledge::update_note(
            &ctx.db,
            &existing.id,
            &sanitized_body,
            tags,
            sources,
            trust,
            None,
            Some(&rel_path),
        )
        .await
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        let front = NoteFrontMatter {
            title: title.to_string(),
            tags: tags.to_vec(),
            sources: sources.to_vec(),
            trust,
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
        let result = reconcile_edges(&ctx.db, &existing.id, title, &wiki_links)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        let mut msg = format!(
            "Updated note '{}' at {}\n\
             Edges: {} created, {} deleted, {} stubs created{index_info}",
            title,
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
        Ok(msg)
    }
}
