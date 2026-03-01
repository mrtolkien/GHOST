use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use tokio::sync::{Mutex, watch};
use tokio::task::JoinHandle;

use regex::Regex;
use std::sync::LazyLock;

use crate::agents::TaskRunner;
use crate::config::Config;
use crate::db;
use crate::db::GhostDb;
use crate::db::sessions::MessageRecord;
use crate::knowledge::parse_note;
use crate::web::slug_from_url;

pub struct ReflectionManager {
    db: GhostDb,
    config: Config,
    task_runner: Arc<TaskRunner>,
    running: Arc<Mutex<()>>,
}

impl ReflectionManager {
    #[must_use]
    pub fn new(db: GhostDb, config: Config, task_runner: Arc<TaskRunner>) -> Self {
        Self {
            db,
            config,
            task_runner,
            running: Arc::new(Mutex::new(())),
        }
    }

    /// Run reflection for a chat session, skipping if no new messages since
    /// the last reflection ran. Fully serialized: dedup check and execution
    /// happen inside the mutex to prevent concurrent reflections.
    pub async fn run_chat_reflection(&self, session_id: &str) {
        let _guard = self.running.lock().await;

        // Skip if no new messages since last reflection
        let state_path = self
            .config
            .workspace
            .join(".state")
            .join("reflection.last.md");
        if state_path.exists()
            && let Ok(metadata) = std::fs::metadata(&state_path)
            && let Ok(modified) = metadata.modified()
        {
            let since: DateTime<Utc> = modified.into();
            match db::sessions::count_messages_since(&self.db, session_id, &since).await {
                Ok(0) => {
                    logfire::debug!(
                        "reflection skipped: no new activity",
                        session_id = session_id.to_string(),
                    );
                    return;
                }
                Ok(_) => {}
                Err(e) => {
                    logfire::warn!(
                        "reflection: failed to check activity",
                        error = e.to_string(),
                    );
                }
            }
        }

        self.run_inner(session_id, "chat-reflection").await;
    }

    /// Run reflection after an agent handoff by continuing the same agent
    /// session. The model keeps its full research context (warm prompt cache,
    /// preserved reasoning chain) and switches to knowledge extraction mode.
    pub async fn run_after_agent_handoff(&self, agent_session_id: &str) {
        let _guard = self.running.lock().await;
        self.run_fork_reflection(agent_session_id).await;
    }

    /// Spawn a background task that polls for idle chat sessions and triggers
    /// reflection. Checks every 60 seconds; fires `run_chat_reflection` when
    /// a session has been idle for `reflection_idle_minutes`.
    pub fn spawn_idle_watcher(
        self: &Arc<Self>,
        mut shutdown: watch::Receiver<bool>,
    ) -> JoinHandle<()> {
        let reflection = Arc::clone(self);
        let idle_minutes = reflection.config.timing.reflection_idle_minutes;
        logfire::info!(
            "reflection idle watcher started",
            idle_minutes = idle_minutes,
        );

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));

            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        reflection.check_idle_sessions().await;
                    }
                    _ = shutdown.changed() => {
                        logfire::info!("reflection idle watcher shutting down");
                        break;
                    }
                }
            }
        })
    }

    /// Scan all active interface sessions and trigger chat reflection for
    /// any that have been idle beyond the configured threshold.
    async fn check_idle_sessions(&self) {
        let sessions = match db::interface_sessions::list_all_interface_sessions(&self.db).await {
            Ok(s) => s,
            Err(e) => {
                logfire::warn!(
                    "reflection watcher: failed to list sessions",
                    error = e.to_string(),
                );
                return;
            }
        };

        let now = Utc::now();
        let idle_threshold =
            chrono::Duration::minutes(self.config.timing.reflection_idle_minutes as i64);

        for record in sessions {
            let session_id = &record.session_id;

            let session = match db::sessions::get_session(&self.db, session_id).await {
                Ok(s) => s,
                Err(e) => {
                    logfire::warn!(
                        "reflection watcher: failed to load session",
                        session_id = session_id.clone(),
                        error = e.to_string(),
                    );
                    continue;
                }
            };

            if session.status != "active" {
                continue;
            }

            let last_activity = chrono::DateTime::parse_from_rfc3339(&session.last_activity_at)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            if now - last_activity < idle_threshold {
                continue;
            }

            self.run_chat_reflection(session_id).await;
        }
    }

    /// Fork reflection: continue the agent's own session with a knowledge
    /// extraction prompt. Assumes the caller holds the mutex.
    #[tracing::instrument(name = "fork reflection", skip_all, fields(
        agent_session_id = agent_session_id,
    ))]
    async fn run_fork_reflection(&self, agent_session_id: &str) {
        logfire::info!(
            "fork reflection started",
            agent_session_id = agent_session_id.to_string(),
        );

        // Classify web cache before reflection for post-processing
        let messages =
            match db::sessions::list_messages_by_session(&self.db, agent_session_id).await {
                Ok(m) => m,
                Err(e) => {
                    logfire::error!(
                        "fork reflection: failed to list messages",
                        error = e.to_string(),
                    );
                    return;
                }
            };
        let agent_findings = extract_agent_findings(&messages);
        let classified = classify_web_cache(
            &self.config.workspace,
            agent_session_id,
            agent_findings.as_deref(),
            1000,
        );

        let prompt = build_fork_reflection_prompt(&self.config.workspace);

        match self
            .task_runner
            .continue_to_completion(agent_session_id, &prompt)
            .await
        {
            Ok((findings, _meta)) => {
                // Save handoff note
                let state_dir = self.config.workspace.join(".state");
                let _ = std::fs::create_dir_all(&state_dir);
                let state_path = state_dir.join("reflection.last.md");
                if let Err(e) = std::fs::write(&state_path, &findings) {
                    logfire::warn!("reflection: failed to write state", error = e.to_string());
                }

                let curation =
                    curate_references(&self.config.workspace, agent_session_id, &classified);
                let cited_count =
                    link_cited_edges(&self.db, &self.config.workspace, &classified).await;

                logfire::info!(
                    "fork reflection completed",
                    agent_session_id = agent_session_id.to_string(),
                    refs_moved = curation.moved,
                    refs_deleted = curation.deleted,
                    cited_edges = cited_count,
                );
            }
            Err(e) => {
                logfire::error!(
                    "fork reflection failed",
                    agent_session_id = agent_session_id.to_string(),
                    error = e.to_string(),
                );
            }
        }
    }

    /// Inner reflection logic — assumes the caller already holds the mutex.
    #[tracing::instrument(name = "run reflection", skip_all, fields(
        session_id = ?session_id,
        agent_name = %agent_name,
    ))]
    async fn run_inner(&self, session_id: &str, agent_name: &str) {
        logfire::info!("reflection started", session_id = session_id.to_string(),);

        // Build user message and capture classified cache files for post-processing
        let (user_message, classified) = match self.build_user_message(session_id).await {
            Ok(result) => result,
            Err(e) => {
                logfire::error!(
                    "reflection: failed to build user message",
                    error = e.to_string(),
                );
                return;
            }
        };

        match self
            .task_runner
            .run_to_completion(agent_name, &user_message, Some(session_id))
            .await
        {
            Ok((findings, _meta)) => {
                // Save handoff note
                let state_dir = self.config.workspace.join(".state");
                let _ = std::fs::create_dir_all(&state_dir);
                let state_path = state_dir.join("reflection.last.md");
                if let Err(e) = std::fs::write(&state_path, &findings) {
                    logfire::warn!("reflection: failed to write state", error = e.to_string(),);
                }

                // Deterministic reference curation (replaces clear_web_cache)
                let curation = curate_references(&self.config.workspace, session_id, &classified);

                // Create cited edges (note → reference) in the knowledge graph
                let cited_count =
                    link_cited_edges(&self.db, &self.config.workspace, &classified).await;

                logfire::info!(
                    "reflection completed",
                    session_id = session_id.to_string(),
                    refs_moved = curation.moved,
                    refs_deleted = curation.deleted,
                    cited_edges = cited_count,
                );
            }
            Err(e) => {
                logfire::error!(
                    "reflection failed",
                    session_id = session_id.to_string(),
                    error = e.to_string(),
                );
            }
        }
    }

    /// Build the user message and return it along with the captured
    /// classified cache files (needed for post-processing).
    async fn build_user_message(
        &self,
        session_id: &str,
    ) -> Result<(String, Vec<ClassifiedCacheFile>), db::DatabaseError> {
        let previous_handoff = load_state_file(&self.config.workspace, "reflection.last.md")
            .unwrap_or_else(|| "No previous handoff.".to_string());

        let diary_today = load_diary_today(&self.config.workspace)
            .unwrap_or_else(|| "No diary entry for today.".to_string());

        let messages = db::sessions::list_messages_by_session(&self.db, session_id).await?;
        let agent_findings = extract_agent_findings(&messages);
        let transcript = filter_transcript(&messages);

        let classified = classify_web_cache(
            &self.config.workspace,
            session_id,
            agent_findings.as_deref(),
            1000,
        );
        let web_cache_section = format_classified_cache(&classified);

        let message = build_reflection_user_message(
            &previous_handoff,
            &diary_today,
            &transcript,
            agent_findings.as_deref(),
            &web_cache_section,
        );

        Ok((message, classified))
    }
}

/// Build the user message for the reflection agent from context variables.
#[must_use]
pub fn build_reflection_user_message(
    previous_handoff: &str,
    diary_today: &str,
    transcript: &str,
    agent_findings: Option<&str>,
    web_cache_files: &str,
) -> String {
    let agent_section = match agent_findings {
        Some(findings) => format!(
            "## Agent Findings\n\
             The research agent produced the following synthesized report. \
             Use this as your primary source of information — it summarizes \
             what was learned from the web cache files below.\n\
             \n\
             {findings}\n\
             \n"
        ),
        None => String::new(),
    };

    format!(
        "## Previous Handoff Note\n\
         {previous_handoff}\n\
         \n\
         ## Today's Diary\n\
         {diary_today}\n\
         \n\
         {agent_section}\
         ## Conversation Transcript (filtered)\n\
         Tool results are stripped — use `read_file` to retrieve content \
         saved during the conversation.\n\
         \n\
         {transcript}\n\
         \n\
         ## Web Cache Files\n\
         {web_cache_files}"
    )
}

/// Extract the last substantial assistant message as "agent findings".
///
/// In agent sessions, the final assistant message typically contains the
/// synthesized research report. This extracts it so reflection can see the
/// report prominently instead of buried in `[assistant]` transcript lines.
///
/// Returns `None` if no assistant message has at least 500 chars of content.
#[must_use]
pub fn extract_agent_findings(messages: &[MessageRecord]) -> Option<String> {
    messages
        .iter()
        .rev()
        .find(|m| m.role == "assistant" && m.content.len() >= 500)
        .map(|m| m.content.clone())
}

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
/// files so reflection has content for writing notes.
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

/// Format classified cache files as structured XML for the reflection prompt.
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

/// Filter a transcript for reflection: preserve user/assistant text,
/// preserve tool call names+inputs, strip tool results.
pub fn filter_transcript(messages: &[MessageRecord]) -> String {
    let mut lines = Vec::new();

    for msg in messages {
        match msg.role.as_str() {
            "user" => {
                // Check for tool results — if present, this is a
                // tool-result message and we skip it
                if msg.tool_results.is_some() {
                    continue;
                }
                if !msg.content.trim().is_empty() {
                    lines.push(format!("[user] {}", msg.content));
                }
            }
            "assistant" => {
                if !msg.content.trim().is_empty() {
                    lines.push(format!("[assistant] {}", msg.content));
                }
                // Include tool call names + brief summary
                if let Some(calls) = msg.tool_calls_parsed() {
                    for call in &calls {
                        let name = call
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown");
                        let input = call
                            .get("input")
                            .map(|v| {
                                let s = v.to_string();
                                if s.len() > 200 {
                                    let end = s.floor_char_boundary(200);
                                    format!("{}...", &s[..end])
                                } else {
                                    s
                                }
                            })
                            .unwrap_or_default();
                        lines.push(format!("[tool_call] {name}({input})"));
                    }
                }
            }
            _ => {}
        }
    }

    lines.join("\n")
}

fn load_state_file(workspace: &Path, filename: &str) -> Option<String> {
    let path = workspace.join(".state").join(filename);
    match std::fs::read_to_string(&path) {
        Ok(content) if !content.trim().is_empty() => Some(content),
        _ => None,
    }
}

fn load_diary_today(workspace: &Path) -> Option<String> {
    let today = Utc::now().format("%Y-%m-%d").to_string();
    let path = workspace.join("diary").join(format!("{today}.md"));
    match std::fs::read_to_string(&path) {
        Ok(content) if !content.trim().is_empty() => Some(content),
        _ => None,
    }
}

/// Build a user message for the fork reflection approach.
///
/// Instead of starting a new agent session, this prompt is sent as a
/// continuation of the existing research session. It instructs the model
/// to switch from research to knowledge extraction mode.
///
/// Reads the note-writer skill from the workspace and inlines it.
#[must_use]
pub fn build_fork_reflection_prompt(workspace: &Path) -> String {
    let skill_path = workspace
        .join("skills")
        .join("note-writer")
        .join("skill.md");
    let skill_body = match std::fs::read_to_string(&skill_path) {
        Ok(content) => strip_skill_frontmatter(&content),
        Err(_) => {
            logfire::warn!(
                "fork reflection: note-writer skill not found",
                path = skill_path.display().to_string(),
            );
            "[note-writer skill not found — create structured notes with wiki links]".to_string()
        }
    };

    format!(
        "Your research phase is complete. Switch to knowledge extraction mode.\n\
         \n\
         **Do NOT search or fetch any more web pages.** Your only job now is to \
         organize what you learned into structured knowledge notes.\n\
         \n\
         A text-only response (no tool calls) ends this session. Do all work \
         through tools.\n\
         \n\
         ## Workflow\n\
         1. Discover existing notes (`run_shell_command` to list notes/, \
         `knowledge_search` to check for duplicates)\n\
         2. Create a TODO plan listing every entity to write notes about\n\
         3. Create notes following the guide below\n\
         4. Verify completeness against your entity list\n\
         5. Handoff (text-only summary of what you created)\n\
         \n\
         ## Note-Writer Guide\n\
         \n\
         {skill_body}"
    )
}

/// Strip YAML frontmatter from a skill file.
fn strip_skill_frontmatter(content: &str) -> String {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return content.to_string();
    }
    let after_open = &trimmed[3..];
    if let Some(close) = after_open.find("\n---") {
        let body_start = close + 4;
        after_open[body_start..]
            .trim_start_matches('\n')
            .to_string()
    } else {
        content.to_string()
    }
}

/// Clear all files in the `.cache/{session_id}/` directory.
pub fn clear_web_cache(workspace: &Path, session_id: &str) -> Result<(), std::io::Error> {
    let cache_dir = workspace.join(".cache").join(session_id);
    if !cache_dir.exists() {
        return Ok(());
    }

    for entry in std::fs::read_dir(&cache_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            std::fs::remove_file(&path)?;
        }
    }

    Ok(())
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
        // Only process files that were used (cited or URL in notes)
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

/// Deterministically curate web cache files after reflection completes.
///
/// Takes the `ClassifiedCacheFile` list captured at prompt-build time (scoped
/// to this session — never re-scans the directory). Scans notes for source
/// URLs, then:
/// - **Used files** (cited in findings OR URL found in notes) → move to
///   `references/{topic}/`
/// - **Unused files from the captured list only** → delete
/// - Files NOT in the captured list → untouched (belong to other sessions)
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
            // Already moved/deleted by the agent or another process
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
            // Move to references
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
        } else {
            // Delete unused file
            if let Err(e) = std::fs::remove_file(&cache_path) {
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
    /// `None` if the note has no tags.
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
///
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

/// Path: `references/{note_topic}/{domain}/{filename}` when a citing note has
/// a topic tag, otherwise `references/{domain}/{filename}`.
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

    fn make_message(
        role: &str,
        content: &str,
        tool_calls: Option<Vec<serde_json::Value>>,
        tool_results: Option<Vec<serde_json::Value>>,
    ) -> MessageRecord {
        MessageRecord {
            id: "test_msg".to_string(),
            session_id: "test_session".to_string(),
            role: role.to_string(),
            content: content.to_string(),
            tool_calls: tool_calls.map(|v| serde_json::to_string(&v).unwrap()),
            tool_results: tool_results.map(|v| serde_json::to_string(&v).unwrap()),
            raw_output: None,
            created_at: crate::db::now(),
        }
    }

    #[test]
    fn transcript_preserves_user_and_assistant_text() {
        let messages = vec![
            make_message("user", "Hello", None, None),
            make_message("assistant", "Hi there!", None, None),
        ];

        let result = filter_transcript(&messages);
        assert!(result.contains("[user] Hello"));
        assert!(result.contains("[assistant] Hi there!"));
    }

    #[test]
    fn transcript_preserves_tool_calls() {
        let tool_call = serde_json::json!({
            "name": "read_file",
            "input": {"path": "/tmp/test.txt"}
        });
        let messages = vec![make_message("assistant", "", Some(vec![tool_call]), None)];

        let result = filter_transcript(&messages);
        assert!(result.contains("[tool_call] read_file("));
        assert!(result.contains("/tmp/test.txt"));
    }

    #[test]
    fn transcript_strips_tool_results() {
        let tool_result = serde_json::json!({
            "tool_use_id": "123",
            "content": "file contents here very long..."
        });
        let messages = vec![
            make_message("user", "Do something", None, None),
            make_message("user", "", None, Some(vec![tool_result])),
        ];

        let result = filter_transcript(&messages);
        assert!(result.contains("[user] Do something"));
        // Tool result message should be stripped
        assert!(!result.contains("file contents here"));
    }

    #[test]
    fn transcript_truncates_long_tool_inputs() {
        let long_input = "x".repeat(300);
        let tool_call = serde_json::json!({
            "name": "write_file",
            "input": {"content": long_input}
        });
        let messages = vec![make_message("assistant", "", Some(vec![tool_call]), None)];

        let result = filter_transcript(&messages);
        assert!(result.contains("..."));
    }

    #[test]
    fn web_cache_clearing() {
        let dir = TempDir::new().unwrap();
        let cache_dir = dir.path().join(".cache").join("test-session");
        std::fs::create_dir_all(&cache_dir).unwrap();

        std::fs::write(cache_dir.join("file1.md"), "content").unwrap();
        std::fs::write(cache_dir.join("file2.md"), "content").unwrap();

        assert!(cache_dir.join("file1.md").exists());

        clear_web_cache(dir.path(), "test-session").unwrap();

        assert!(!cache_dir.join("file1.md").exists());
        assert!(!cache_dir.join("file2.md").exists());
        // Directory itself should still exist
        assert!(cache_dir.exists());
    }

    #[test]
    fn web_cache_clearing_missing_dir_is_ok() {
        let dir = TempDir::new().unwrap();
        assert!(clear_web_cache(dir.path(), "nonexistent").is_ok());
    }

    #[test]
    fn load_state_file_returns_content() {
        let dir = TempDir::new().unwrap();
        let state_dir = dir.path().join(".state");
        std::fs::create_dir_all(&state_dir).unwrap();
        std::fs::write(state_dir.join("test.md"), "handoff note").unwrap();

        let result = load_state_file(dir.path(), "test.md");
        assert_eq!(result, Some("handoff note".to_string()));
    }

    #[test]
    fn load_state_file_returns_none_when_missing() {
        let dir = TempDir::new().unwrap();
        let result = load_state_file(dir.path(), "nonexistent.md");
        assert_eq!(result, None);
    }

    #[test]
    fn load_state_file_returns_none_when_empty() {
        let dir = TempDir::new().unwrap();
        let state_dir = dir.path().join(".state");
        std::fs::create_dir_all(&state_dir).unwrap();
        std::fs::write(state_dir.join("empty.md"), "  \n  ").unwrap();

        let result = load_state_file(dir.path(), "empty.md");
        assert_eq!(result, None);
    }

    #[test]
    fn build_user_message_includes_all_sections() {
        let msg = build_reflection_user_message(
            "Previous handoff content",
            "Diary entry today",
            "[user] Hello\n[assistant] Hi",
            None,
            "file1.md\nfile2.md",
        );
        assert!(msg.contains("## Previous Handoff Note"));
        assert!(msg.contains("Previous handoff content"));
        assert!(msg.contains("## Today's Diary"));
        assert!(msg.contains("Diary entry today"));
        assert!(msg.contains("## Conversation Transcript"));
        assert!(msg.contains("[user] Hello"));
        assert!(msg.contains("## Web Cache Files"));
        assert!(msg.contains("file1.md"));
        // No agent findings section when None
        assert!(!msg.contains("## Agent Findings"));
    }

    #[test]
    fn build_user_message_with_agent_findings() {
        let msg = build_reflection_user_message(
            "No previous handoff.",
            "No diary entry for today.",
            "[user] research X",
            Some("The research found that X is better than Y because..."),
            "file1.md",
        );
        assert!(msg.contains("## Agent Findings"));
        assert!(msg.contains("X is better than Y"));
    }

    #[test]
    fn build_user_message_defaults() {
        let msg = build_reflection_user_message(
            "No previous handoff.",
            "No diary entry for today.",
            "",
            None,
            "No cached files.",
        );
        assert!(msg.contains("No previous handoff."));
        assert!(msg.contains("No diary entry for today."));
        assert!(msg.contains("No cached files."));
    }

    #[test]
    fn extract_agent_findings_picks_last_long_message() {
        let messages = vec![
            make_message("user", "Research something", None, None),
            make_message("assistant", "Short reply", None, None),
            make_message("assistant", &"x".repeat(600), None, None),
        ];
        let findings = extract_agent_findings(&messages);
        assert!(findings.is_some());
        assert_eq!(findings.unwrap().len(), 600);
    }

    #[test]
    fn extract_agent_findings_returns_none_for_short_messages() {
        let messages = vec![
            make_message("user", "Hello", None, None),
            make_message("assistant", "Short", None, None),
        ];
        assert!(extract_agent_findings(&messages).is_none());
    }

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

        // Write a cache file with a URL in frontmatter
        std::fs::write(
            cache_dir.join("2026-02-23T07-36-01_tomshardware-com-reviews-bambu-lab-p1s.md"),
            "---\nurl: https://www.tomshardware.com/reviews/bambu-lab-p1s\nfetched_at: 2026-02-23\n---\n\n# Review\nGreat printer.\n",
        ).unwrap();

        // Write an uncited cache file
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

        // Create cache files
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

        // Cited file moved to references/
        assert!(!cache_dir.join("cited.md").exists());
        assert!(dir.path().join("references/example-com/cited.md").exists());

        // Uncited file deleted
        assert!(!cache_dir.join("uncited.md").exists());
    }

    #[test]
    fn curate_references_skips_files_not_in_list() {
        let dir = TempDir::new().unwrap();
        let cache_dir = dir.path().join(".cache").join("test-session");
        std::fs::create_dir_all(&cache_dir).unwrap();

        // File from another session — not in the classified list
        std::fs::write(cache_dir.join("other_session.md"), "content").unwrap();
        // File from our session
        std::fs::write(cache_dir.join("our_file.md"), "content").unwrap();

        let classified = vec![ClassifiedCacheFile {
            filename: "our_file.md".to_string(),
            url: String::new(),
            cited: false,
            is_search: true,
            preview: None,
        }];

        let result = curate_references(dir.path(), "test-session", &classified);
        assert_eq!(result.deleted, 1);

        // Other session's file untouched
        assert!(cache_dir.join("other_session.md").exists());
        // Our file deleted
        assert!(!cache_dir.join("our_file.md").exists());
    }

    #[test]
    fn curate_references_url_in_notes_marks_as_used() {
        let dir = TempDir::new().unwrap();
        let cache_dir = dir.path().join(".cache").join("test-session");
        let notes_dir = dir.path().join("notes");
        std::fs::create_dir_all(&cache_dir).unwrap();
        std::fs::create_dir_all(&notes_dir).unwrap();

        // Cache file not cited in agent findings
        std::fs::write(
            cache_dir.join("article.md"),
            "---\nurl: https://example.com/article\n---\n# Content\n",
        )
        .unwrap();

        // But its URL appears in a note body
        std::fs::write(
            notes_dir.join("test_note.md"),
            "Some note body.\nSource: https://example.com/article\n",
        )
        .unwrap();

        let classified = vec![ClassifiedCacheFile {
            filename: "article.md".to_string(),
            url: "https://example.com/article".to_string(),
            cited: false, // Not cited in findings
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

        // Note with a tag → provides topic context
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

        // Should be scoped under the note's first tag segment
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

        // Note with sources in frontmatter
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
