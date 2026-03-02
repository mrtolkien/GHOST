use std::path::Path;

use regex::Regex;
use std::sync::LazyLock;

use crate::db;
use crate::db::GhostDb;
use crate::knowledge::parse_note;

use super::slug_from_url;

/// A web cache file classified by whether it was cited in agent findings.
#[derive(Debug, Clone)]
pub struct ClassifiedCacheFile {
    pub filename: String,
    pub url: String,
    pub cited: bool,
    /// True if this is a search-result listing (has `query:` frontmatter).
    pub is_search: bool,
    /// First N chars of the file content (for cited files).
    pub preview: Option<String>,
}

static URL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"https?://[^\s\]\)>,]+").expect("url regex"));

/// Extract URLs from the `## Sources` section of agent findings.
#[must_use]
pub fn extract_source_urls(agent_findings: &str) -> Vec<String> {
    // Find the Sources section — look for "## Sources" or "Sources:" heading
    let sources_start = agent_findings
        .find("## Sources")
        .or_else(|| agent_findings.find("## sources"))
        .or_else(|| agent_findings.rfind("Sources:\n"));

    let section = match sources_start {
        Some(pos) => &agent_findings[pos..],
        None => return Vec::new(),
    };

    URL_RE
        .find_iter(section)
        .map(|m| m.as_str().trim_end_matches(|c: char| ".,;:)".contains(c)))
        .map(String::from)
        .collect()
}

/// Classify web cache files as cited or uncited based on agent findings.
///
/// Matches URLs from the findings' Sources section against cache filenames
/// using `slug_from_url`. Also reads the first `preview_chars` of cited
/// files so the agent has content for writing notes.
#[must_use]
pub fn classify_web_cache(
    workspace: &Path,
    session_id: &str,
    agent_findings: Option<&str>,
    preview_chars: usize,
) -> Vec<ClassifiedCacheFile> {
    let cache_dir = workspace.join(".cache").join(session_id);
    if !cache_dir.exists() {
        return Vec::new();
    }

    let source_urls = agent_findings.map(extract_source_urls).unwrap_or_default();

    // Build a set of URL slugs for matching
    let source_slugs: Vec<String> = source_urls.iter().map(|u| slug_from_url(u)).collect();

    let mut entries: Vec<_> = match std::fs::read_dir(&cache_dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .extension()
                    .and_then(|x| x.to_str())
                    .is_some_and(|x| x == "md")
            })
            .collect(),
        Err(_) => return Vec::new(),
    };
    entries.sort_by_key(|e| e.path());

    entries
        .iter()
        .map(|entry| {
            let path = entry.path();
            let filename = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string();

            // Extract URL and type from frontmatter
            let content = std::fs::read_to_string(&path).unwrap_or_default();
            let (url, is_search) = extract_frontmatter_info(&content);

            // Check if this file's URL slug matches any source URL slug
            let file_slug = if !url.is_empty() {
                slug_from_url(&url)
            } else {
                // Fallback: extract slug from filename (strip timestamp prefix)
                filename
                    .split('_')
                    .skip(1)
                    .collect::<Vec<_>>()
                    .join("_")
                    .trim_end_matches(".md")
                    .to_string()
            };

            let cited = source_slugs.iter().any(|slug| {
                slug == &file_slug || file_slug.starts_with(slug) || slug.starts_with(&file_slug)
            });

            let preview = if cited && preview_chars > 0 {
                // Skip frontmatter, take first N chars of body
                let body_start = content
                    .find("---")
                    .and_then(|first| {
                        content[first + 3..]
                            .find("---")
                            .map(|second| first + 3 + second + 3)
                    })
                    .unwrap_or(0);
                let body = content[body_start..].trim_start();
                Some(body.chars().take(preview_chars).collect())
            } else {
                None
            };

            ClassifiedCacheFile {
                filename,
                url,
                cited,
                is_search,
                preview,
            }
        })
        .collect()
}

/// Extract URL and whether it's a search result from frontmatter.
/// Returns (url_or_empty, is_search).
fn extract_frontmatter_info(content: &str) -> (String, bool) {
    let mut in_frontmatter = false;
    let mut url = String::new();
    let mut is_search = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "---" {
            if in_frontmatter {
                break;
            }
            in_frontmatter = true;
            continue;
        }
        if in_frontmatter {
            if let Some(u) = trimmed.strip_prefix("url: ") {
                url = u.to_string();
            } else if trimmed.starts_with("query: ") {
                is_search = true;
            }
        }
    }
    (url, is_search)
}

/// Format classified cache files as structured XML for agent prompts.
///
/// Cited fetch files include a content preview. Uncited/search files are
/// self-closing elements. The XML format lets the agent see sources clearly
/// without needing to manage them — reference curation happens deterministically
/// in post-processing.
#[must_use]
pub fn format_classified_cache(files: &[ClassifiedCacheFile]) -> String {
    if files.is_empty() {
        return "No cached files.".to_string();
    }

    let mut output = String::from("<web-cache>\n");

    for file in files {
        let file_type = if file.is_search { "search" } else { "fetch" };
        let cited = if file.cited { "true" } else { "false" };

        // Escape XML special chars in URL
        let url = xml_escape(&file.url);

        if let Some(preview) = &file.preview {
            output.push_str(&format!(
                "  <file filename=\"{filename}\" url=\"{url}\" \
                 type=\"{file_type}\" cited=\"{cited}\">\n",
                filename = xml_escape(&file.filename),
            ));
            // Indent preview content
            for line in preview.lines() {
                output.push_str(&format!("    {line}\n"));
            }
            output.push_str("  </file>\n");
        } else {
            output.push_str(&format!(
                "  <file filename=\"{filename}\" url=\"{url}\" \
                 type=\"{file_type}\" cited=\"{cited}\" />\n",
                filename = xml_escape(&file.filename),
            ));
        }
    }

    output.push_str("</web-cache>");
    output
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Create `cited` graph edges (note → reference) for notes that cite
/// URLs matching moved reference files.
///
/// Returns the number of edges created.
#[tracing::instrument(skip_all, fields(classified_count = classified.len()))]
pub async fn link_cited_edges(
    db: &GhostDb,
    workspace: &Path,
    classified: &[ClassifiedCacheFile],
) -> usize {
    let note_urls = collect_note_source_urls(workspace);
    let mut created: usize = 0;

    for file in classified {
        if file.url.is_empty() || file.is_search {
            continue;
        }

        // Find or create the reference record for this URL
        let domain = topic_from_url(&file.url);
        let rel_path = find_reference_on_disk(workspace, &domain, &file.filename);
        let Some(rel_path) = rel_path else {
            continue;
        };
        let topic = rel_path
            .strip_prefix("references/")
            .and_then(|r| r.split('/').next())
            .unwrap_or(&domain)
            .to_string();
        let ref_record = match db::knowledge::find_reference_by_url(db, &file.url).await {
            Ok(Some(r)) => r,
            Ok(None) => {
                // Try by path
                match db::knowledge::find_reference_by_path(db, &rel_path).await {
                    Ok(Some(r)) => r,
                    _ => {
                        // File was moved to disk by curate_references but no
                        // DB record exists yet — create one now.
                        let ref_file = workspace.join(&rel_path);
                        let content = std::fs::read_to_string(&ref_file).unwrap_or_default();
                        let preview: String = content.chars().take(2000).collect();
                        match db::knowledge::create_reference(
                            db,
                            &topic,
                            &rel_path,
                            &preview,
                            Some(&file.url),
                        )
                        .await
                        {
                            Ok(id) => match db::knowledge::get_reference(db, &id).await {
                                Ok(r) => r,
                                Err(_) => continue,
                            },
                            Err(e) => {
                                logfire::warn!(
                                    "link_cited_edges: failed to create reference",
                                    url = file.url.clone(),
                                    error = e.to_string(),
                                );
                                continue;
                            }
                        }
                    }
                }
            }
            Err(e) => {
                logfire::warn!(
                    "link_cited_edges: failed to find reference",
                    url = file.url.clone(),
                    error = e.to_string(),
                );
                continue;
            }
        };

        // Find notes that cite this URL (via frontmatter sources field)
        for (note_title, source_urls) in &note_urls {
            let cites = source_urls
                .iter()
                .any(|note_url| urls_match(note_url, &file.url));
            if !cites {
                continue;
            }

            // Look up the note record
            let note_record = match db::knowledge::find_note_by_title(db, note_title).await {
                Ok(Some(n)) => n,
                _ => continue,
            };

            match db::knowledge::create_cited_edge(db, &note_record.id, &ref_record.id).await {
                Ok(_) => created += 1,
                Err(e) => {
                    logfire::warn!(
                        "link_cited_edges: failed to create edge",
                        note = note_title.clone(),
                        ref_id = ref_record.id.clone(),
                        error = e.to_string(),
                    );
                }
            }
        }
    }

    logfire::info!("link_cited_edges: done", edges_created = created);
    created
}

/// Collect note titles mapped to their frontmatter source URLs.
fn collect_note_source_urls(workspace: &Path) -> Vec<(String, Vec<String>)> {
    let notes_dir = workspace.join("notes");
    if !notes_dir.exists() {
        return Vec::new();
    }

    let mut result = Vec::new();
    collect_note_sources_recursive(&notes_dir, &mut result);
    result
}

fn collect_note_sources_recursive(dir: &Path, out: &mut Vec<(String, Vec<String>)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_note_sources_recursive(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("md")
            && let Ok(content) = std::fs::read_to_string(&path)
            && let Ok(parsed) = parse_note(&content)
            && !parsed.front.sources.is_empty()
        {
            out.push((parsed.front.title, parsed.front.sources));
        }
    }
}

/// Deterministically curate web cache files after an agent completes.
///
/// Operates only on the `ClassifiedCacheFile` list captured at prompt-build
/// time — files added to the cache dir after the snapshot are left alone.
///
/// - **Used files** (cited in findings OR URL found in notes) → move to
///   `references/{topic}/`
/// - **Unused files** → delete
#[tracing::instrument(skip_all, fields(total = classified.len()))]
pub fn curate_references(
    workspace: &Path,
    session_id: &str,
    classified: &[ClassifiedCacheFile],
) -> CurationResult {
    let mut result = CurationResult::default();

    if classified.is_empty() {
        return result;
    }

    // Collect all URLs found in note bodies
    let note_urls = collect_note_urls(workspace);

    let cache_dir = workspace.join(".cache").join(session_id);

    for file in classified {
        let cache_path = cache_dir.join(&file.filename);
        if !cache_path.exists() {
            continue;
        }

        // A file is "used" if it was cited in agent findings OR its URL
        // appears in any note body
        let matching_note = if file.url.is_empty() {
            None
        } else {
            note_urls.iter().find(|nu| urls_match(&nu.url, &file.url))
        };
        let url_in_notes = matching_note.is_some();
        let used = file.cited || url_in_notes;

        // Topic from the first note that cites this URL (for reference path scoping)
        let note_topic = matching_note.and_then(|nu| nu.topic.clone());

        if used && !file.url.is_empty() {
            match move_to_references(workspace, &cache_path, file, note_topic.as_deref()) {
                Ok(dest) => {
                    logfire::info!(
                        "curate_references: moved",
                        filename = file.filename.clone(),
                        dest = dest,
                    );
                    result.moved += 1;
                }
                Err(e) => {
                    logfire::warn!(
                        "curate_references: move failed",
                        filename = file.filename.clone(),
                        error = e.to_string(),
                    );
                }
            }
        } else if let Err(e) = std::fs::remove_file(&cache_path) {
            logfire::warn!(
                "curate_references: delete failed",
                filename = file.filename.clone(),
                error = e.to_string(),
            );
        } else {
            logfire::info!(
                "curate_references: deleted",
                filename = file.filename.clone(),
            );
            result.deleted += 1;
        }
    }

    logfire::info!(
        "curate_references: done",
        moved = result.moved,
        deleted = result.deleted,
    );

    result
}

/// Result of deterministic reference curation.
#[derive(Debug, Default)]
pub struct CurationResult {
    pub moved: usize,
    pub deleted: usize,
}

/// A URL found in a note, with the note's first tag segment for topic scoping.
#[derive(Debug)]
struct NoteUrl {
    url: String,
    /// First segment of the note's first tag (e.g. "3dprinting" from "3dprinting/printers").
    topic: Option<String>,
}

/// Collect all URLs found in notes under `notes/`, with topic context.
fn collect_note_urls(workspace: &Path) -> Vec<NoteUrl> {
    let notes_dir = workspace.join("notes");
    if !notes_dir.exists() {
        return Vec::new();
    }

    let mut urls = Vec::new();
    collect_urls_recursive(&notes_dir, &mut urls);
    urls
}

fn collect_urls_recursive(dir: &Path, urls: &mut Vec<NoteUrl>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_urls_recursive(&path, urls);
        } else if path.extension().and_then(|e| e.to_str()) == Some("md")
            && let Ok(content) = std::fs::read_to_string(&path)
        {
            let topic = parse_note(&content).ok().and_then(|parsed| {
                parsed
                    .front
                    .tags
                    .first()
                    .and_then(|tag| tag.split('/').next().map(String::from))
            });

            // Extract URLs from frontmatter sources field
            if let Ok(parsed) = parse_note(&content) {
                for source_url in &parsed.front.sources {
                    urls.push(NoteUrl {
                        url: source_url.clone(),
                        topic: topic.clone(),
                    });
                }
            }
            // Also extract bare URLs from body text
            for m in URL_RE.find_iter(&content) {
                let url = m.as_str().trim_end_matches(|c: char| ".,;:)".contains(c));
                urls.push(NoteUrl {
                    url: url.to_string(),
                    topic: topic.clone(),
                });
            }
        }
    }
}

/// Check if two URLs refer to the same page by comparing their slugs.
fn urls_match(a: &str, b: &str) -> bool {
    let slug_a = slug_from_url(a);
    let slug_b = slug_from_url(b);
    slug_a == slug_b || slug_a.starts_with(&slug_b) || slug_b.starts_with(&slug_a)
}

/// Move a cache file to the references directory.
fn move_to_references(
    workspace: &Path,
    cache_path: &Path,
    file: &ClassifiedCacheFile,
    note_topic: Option<&str>,
) -> Result<String, std::io::Error> {
    let domain = topic_from_url(&file.url);
    let dest_dir = match note_topic {
        Some(topic) => workspace.join("references").join(topic).join(&domain),
        None => workspace.join("references").join(&domain),
    };
    std::fs::create_dir_all(&dest_dir)?;

    let dest_path = dest_dir.join(&file.filename);
    std::fs::rename(cache_path, &dest_path)?;

    let rel = match note_topic {
        Some(topic) => format!("references/{topic}/{domain}/{}", file.filename),
        None => format!("references/{domain}/{}", file.filename),
    };
    Ok(rel)
}

/// Find a reference file on disk, checking topic-scoped paths first.
///
/// Searches `references/**/{domain}/{filename}` and falls back to
/// `references/{domain}/{filename}`.
fn find_reference_on_disk(workspace: &Path, domain: &str, filename: &str) -> Option<String> {
    let refs_dir = workspace.join("references");
    if !refs_dir.exists() {
        return None;
    }
    // Check topic-scoped paths: references/{topic}/{domain}/{filename}
    if let Ok(entries) = std::fs::read_dir(&refs_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let candidate = path.join(domain).join(filename);
                if candidate.exists() {
                    let topic = entry.file_name();
                    return Some(format!(
                        "references/{}/{domain}/{filename}",
                        topic.to_string_lossy()
                    ));
                }
            }
        }
    }
    // Fallback: references/{domain}/{filename}
    let flat = refs_dir.join(domain).join(filename);
    if flat.exists() {
        return Some(format!("references/{domain}/{filename}"));
    }
    None
}

/// Extract a topic directory name from a URL's domain.
///
/// E.g. `https://www.tomshardware.com/reviews/...` → `tomshardware-com`
fn topic_from_url(url: &str) -> String {
    let stripped = url
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("www.");

    // Take just the domain part (up to first '/')
    let domain = stripped.split('/').next().unwrap_or(stripped);

    // Replace dots with hyphens for directory name
    domain.replace('.', "-")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn extract_source_urls_from_sources_section() {
        let findings = "\
## Summary\nSome summary.\n\n\
## Sources\n\
1. https://tomshardware.com/reviews/bambu-lab-p1s — detailed review\n\
2. https://all3dp.com/1/best-enclosed-3d-printers/ — comparison\n\
3. https://www.prusa3d.com/product/prusa-core-one — product page\n";

        let urls = extract_source_urls(findings);
        assert_eq!(urls.len(), 3);
        assert!(urls[0].contains("tomshardware.com"));
        assert!(urls[1].contains("all3dp.com"));
        assert!(urls[2].contains("prusa3d.com"));
    }

    #[test]
    fn extract_source_urls_no_sources_section() {
        let findings = "## Summary\nJust a summary, no sources.\n";
        assert!(extract_source_urls(findings).is_empty());
    }

    #[test]
    fn classify_web_cache_matches_cited() {
        let dir = TempDir::new().unwrap();
        let cache_dir = dir.path().join(".cache").join("test-session");
        std::fs::create_dir_all(&cache_dir).unwrap();

        std::fs::write(
            cache_dir.join("2026-02-23T07-36-01_tomshardware-com-reviews-bambu-lab-p1s.md"),
            "---\nurl: https://www.tomshardware.com/reviews/bambu-lab-p1s\nfetched_at: 2026-02-23\n---\n\n# Review\nGreat printer.\n",
        ).unwrap();

        std::fs::write(
            cache_dir.join("2026-02-23T07-35-33_search-results.md"),
            "---\nquery: best printers\nsearched_at: 2026-02-23\n---\n\n1. Result\n",
        )
        .unwrap();

        let findings =
            "## Sources\n1. https://www.tomshardware.com/reviews/bambu-lab-p1s — review\n";
        let classified = classify_web_cache(dir.path(), "test-session", Some(findings), 500);

        assert_eq!(classified.len(), 2);
        let cited: Vec<_> = classified.iter().filter(|f| f.cited).collect();
        let uncited: Vec<_> = classified.iter().filter(|f| !f.cited).collect();

        assert_eq!(cited.len(), 1);
        assert!(cited[0].filename.contains("tomshardware"));
        assert!(cited[0].preview.is_some());
        assert!(cited[0].preview.as_ref().unwrap().contains("Review"));

        assert_eq!(uncited.len(), 1);
        assert!(uncited[0].preview.is_none());
    }

    #[test]
    fn classify_web_cache_empty_dir() {
        let dir = TempDir::new().unwrap();
        let result = classify_web_cache(dir.path(), "test-session", Some("no sources"), 500);
        assert!(result.is_empty());
    }

    #[test]
    fn format_classified_cache_xml() {
        let files = vec![
            ClassifiedCacheFile {
                filename: "cited.md".to_string(),
                url: "https://example.com/cited".to_string(),
                cited: true,
                is_search: false,
                preview: Some("# Title\nContent here".to_string()),
            },
            ClassifiedCacheFile {
                filename: "uncited.md".to_string(),
                url: "https://example.com/uncited".to_string(),
                cited: false,
                is_search: false,
                preview: None,
            },
            ClassifiedCacheFile {
                filename: "search.md".to_string(),
                url: String::new(),
                cited: false,
                is_search: true,
                preview: None,
            },
        ];

        let output = format_classified_cache(&files);
        assert!(output.starts_with("<web-cache>"));
        assert!(output.ends_with("</web-cache>"));
        assert!(output.contains("cited.md"));
        assert!(output.contains("type=\"fetch\" cited=\"true\""));
        assert!(output.contains("# Title"));
        assert!(output.contains("uncited.md"));
        assert!(output.contains("type=\"fetch\" cited=\"false\" />"));
        assert!(output.contains("type=\"search\" cited=\"false\" />"));
    }

    #[test]
    fn format_classified_cache_empty() {
        let output = format_classified_cache(&[]);
        assert_eq!(output, "No cached files.");
    }

    #[test]
    fn curate_references_moves_cited_deletes_uncited() {
        let dir = TempDir::new().unwrap();
        let cache_dir = dir.path().join(".cache").join("test-session");
        std::fs::create_dir_all(&cache_dir).unwrap();

        std::fs::write(
            cache_dir.join("cited.md"),
            "---\nurl: https://example.com/article\n---\n# Article\n",
        )
        .unwrap();
        std::fs::write(
            cache_dir.join("uncited.md"),
            "---\nquery: search query\n---\n1. Result\n",
        )
        .unwrap();

        let classified = vec![
            ClassifiedCacheFile {
                filename: "cited.md".to_string(),
                url: "https://example.com/article".to_string(),
                cited: true,
                is_search: false,
                preview: Some("# Article".to_string()),
            },
            ClassifiedCacheFile {
                filename: "uncited.md".to_string(),
                url: String::new(),
                cited: false,
                is_search: true,
                preview: None,
            },
        ];

        let result = curate_references(dir.path(), "test-session", &classified);
        assert_eq!(result.moved, 1);
        assert_eq!(result.deleted, 1);

        assert!(!cache_dir.join("cited.md").exists());
        assert!(dir.path().join("references/example-com/cited.md").exists());
        assert!(!cache_dir.join("uncited.md").exists());
    }

    #[test]
    fn curate_references_url_in_notes_marks_as_used() {
        let dir = TempDir::new().unwrap();
        let cache_dir = dir.path().join(".cache").join("test-session");
        let notes_dir = dir.path().join("notes");
        std::fs::create_dir_all(&cache_dir).unwrap();
        std::fs::create_dir_all(&notes_dir).unwrap();

        std::fs::write(
            cache_dir.join("article.md"),
            "---\nurl: https://example.com/article\n---\n# Content\n",
        )
        .unwrap();

        std::fs::write(
            notes_dir.join("test_note.md"),
            "Some note body.\nSource: https://example.com/article\n",
        )
        .unwrap();

        let classified = vec![ClassifiedCacheFile {
            filename: "article.md".to_string(),
            url: "https://example.com/article".to_string(),
            cited: false,
            is_search: false,
            preview: None,
        }];

        let result = curate_references(dir.path(), "test-session", &classified);
        assert_eq!(result.moved, 1, "URL in notes should trigger move");
        assert!(!cache_dir.join("article.md").exists());
        assert!(
            dir.path()
                .join("references/example-com/article.md")
                .exists()
        );
    }

    #[test]
    fn curate_references_scopes_under_note_topic() {
        let dir = TempDir::new().unwrap();
        let cache_dir = dir.path().join(".cache").join("test-session");
        let notes_dir = dir.path().join("notes/rust");
        std::fs::create_dir_all(&cache_dir).unwrap();
        std::fs::create_dir_all(&notes_dir).unwrap();

        std::fs::write(
            cache_dir.join("article.md"),
            "---\nurl: https://docs.rs/tokio\n---\n# Tokio docs\n",
        )
        .unwrap();

        std::fs::write(
            notes_dir.join("tokio.md"),
            "---\ntitle: Tokio\ntags:\n  - rust/async\nsources:\n  - https://docs.rs/tokio\n---\nAn async runtime.\n",
        )
        .unwrap();

        let classified = vec![ClassifiedCacheFile {
            filename: "article.md".to_string(),
            url: "https://docs.rs/tokio".to_string(),
            cited: false,
            is_search: false,
            preview: None,
        }];

        let result = curate_references(dir.path(), "test-session", &classified);
        assert_eq!(result.moved, 1);
        assert!(
            dir.path()
                .join("references/rust/docs-rs/article.md")
                .exists(),
            "reference should be under references/rust/docs-rs/"
        );
    }

    #[test]
    fn collect_note_urls_finds_frontmatter_sources() {
        let dir = TempDir::new().unwrap();
        let notes_dir = dir.path().join("notes");
        std::fs::create_dir_all(&notes_dir).unwrap();

        std::fs::write(
            notes_dir.join("test.md"),
            "---\ntitle: Test\nsources:\n  - https://example.com/src1\n  - https://other.com/src2\n---\nBody text with no URLs.\n",
        )
        .unwrap();

        let urls = collect_note_urls(dir.path());
        assert!(
            urls.iter().any(|u| u.url.contains("example.com/src1")),
            "should find frontmatter source URL: {urls:?}"
        );
        assert!(
            urls.iter().any(|u| u.url.contains("other.com/src2")),
            "should find second frontmatter source URL: {urls:?}"
        );
    }

    #[test]
    fn topic_from_url_extracts_domain() {
        assert_eq!(
            topic_from_url("https://www.tomshardware.com/reviews/test"),
            "tomshardware-com"
        );
        assert_eq!(
            topic_from_url("https://all3dp.com/best-printers"),
            "all3dp-com"
        );
        assert_eq!(topic_from_url("http://example.org/page"), "example-org");
    }
}
