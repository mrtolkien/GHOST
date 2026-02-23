use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use surrealdb::Surreal;
use surrealdb::engine::local::Db;
use surrealdb::sql::Thing;
use tokio::sync::Mutex;

use regex::Regex;
use std::sync::LazyLock;

use crate::agents::TaskRunner;
use crate::config::Config;
use crate::db;
use crate::db::sessions::MessageRecord;
use crate::web::slug_from_url;

pub struct ReflectionManager {
    db: Surreal<Db>,
    config: Config,
    task_runner: Arc<TaskRunner>,
    running: Arc<Mutex<()>>,
}

impl ReflectionManager {
    #[must_use]
    pub fn new(db: Surreal<Db>, config: Config, task_runner: Arc<TaskRunner>) -> Self {
        Self {
            db,
            config,
            task_runner,
            running: Arc::new(Mutex::new(())),
        }
    }

    /// Run reflection after a heartbeat, with delay and skip logic.
    #[tracing::instrument(skip_all, fields(session_id = %session_id))]
    pub async fn run_after_heartbeat(&self, session_id: &str, session_thing: &Thing) {
        // Delay before running
        let delay = Duration::from_secs(self.config.timing.reflection_idle_minutes * 60);
        tokio::time::sleep(delay).await;

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
            match db::sessions::count_messages_since(&self.db, session_thing, &since).await {
                Ok(0) => {
                    logfire::info!(
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

        self.run(session_id, session_thing).await;
    }

    /// Run reflection on reboot — always runs, no skip logic.
    #[tracing::instrument(skip_all, fields(session_id = %session_id))]
    pub async fn run_on_reboot(&self, session_id: &str, session_thing: &Thing) {
        self.run(session_id, session_thing).await;
    }

    #[tracing::instrument(skip_all, fields(session_id = %session_id))]
    async fn run(&self, session_id: &str, session_thing: &Thing) {
        // Only one reflection at a time
        let _guard = match self.running.try_lock() {
            Ok(guard) => guard,
            Err(_) => {
                logfire::info!(
                    "reflection skipped: already running",
                    session_id = session_id.to_string(),
                );
                return;
            }
        };

        logfire::info!("reflection started", session_id = session_id.to_string(),);

        // Build user message from context variables
        let user_message = match self.build_user_message(session_thing).await {
            Ok(msg) => msg,
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
            .run_to_completion("reflection", &user_message, Some(session_thing))
            .await
        {
            Ok(findings) => {
                // Save handoff note
                let state_dir = self.config.workspace.join(".state");
                let _ = std::fs::create_dir_all(&state_dir);
                let state_path = state_dir.join("reflection.last.md");
                if let Err(e) = std::fs::write(&state_path, &findings) {
                    logfire::warn!("reflection: failed to write state", error = e.to_string(),);
                }

                // Clear web cache on success
                if let Err(e) = clear_web_cache(&self.config.workspace) {
                    logfire::warn!(
                        "reflection: failed to clear web cache",
                        error = e.to_string(),
                    );
                }

                logfire::info!("reflection completed", session_id = session_id.to_string(),);
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

    async fn build_user_message(&self, session_thing: &Thing) -> Result<String, db::DatabaseError> {
        let previous_handoff = load_state_file(&self.config.workspace, "reflection.last.md")
            .unwrap_or_else(|| "No previous handoff.".to_string());

        let diary_today = load_diary_today(&self.config.workspace)
            .unwrap_or_else(|| "No diary entry for today.".to_string());

        let messages = db::sessions::list_messages_by_session(&self.db, session_thing).await?;
        let agent_findings = extract_agent_findings(&messages);
        let transcript = filter_transcript(&messages);

        let classified =
            classify_web_cache(&self.config.workspace, agent_findings.as_deref(), 1000);
        let web_cache_section = format_classified_cache(&classified);

        Ok(build_reflection_user_message(
            &previous_handoff,
            &diary_today,
            &transcript,
            agent_findings.as_deref(),
            &web_cache_section,
        ))
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
    agent_findings: Option<&str>,
    preview_chars: usize,
) -> Vec<ClassifiedCacheFile> {
    let cache_dir = workspace.join(".web-cache");
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

            // Extract URL from frontmatter
            let content = std::fs::read_to_string(&path).unwrap_or_default();
            let url = extract_frontmatter_url(&content);

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
                preview,
            }
        })
        .collect()
}

fn extract_frontmatter_url(content: &str) -> String {
    let mut in_frontmatter = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "---" {
            if in_frontmatter {
                break;
            }
            in_frontmatter = true;
            continue;
        }
        if in_frontmatter && let Some(url) = trimmed.strip_prefix("url: ") {
            return url.to_string();
        }
    }
    String::new()
}

/// Format classified cache files into sections for the reflection prompt.
#[must_use]
pub fn format_classified_cache(files: &[ClassifiedCacheFile]) -> String {
    let cited: Vec<_> = files.iter().filter(|f| f.cited).collect();
    let uncited: Vec<_> = files.iter().filter(|f| !f.cited).collect();

    let mut output = String::new();

    if !cited.is_empty() {
        output.push_str("### Cited in Agent Report (move to references)\n\n");
        output.push_str(
            "These files were cited in the agent's research report. \
             Move them with `reference_manage` and link to them from notes.\n\n",
        );
        for file in &cited {
            output.push_str(&format!(
                "- `.web-cache/{filename}` — {url}\n",
                filename = file.filename,
                url = file.url,
            ));
            if let Some(preview) = &file.preview {
                // Indent preview as a block
                output.push_str(&format!(
                    "  > {preview}\n",
                    preview = preview.lines().next().unwrap_or("")
                ));
            }
        }
    }

    if !uncited.is_empty() {
        if !cited.is_empty() {
            output.push('\n');
        }
        output.push_str("### Uncited (review and decide)\n\n");
        output.push_str(
            "These files were NOT cited in the agent's report. \
             Review filenames — delete junk (403 pages, empty, search result listings) \
             and move anything useful.\n\n",
        );
        for file in &uncited {
            output.push_str(&format!(
                "- `.web-cache/{filename}` — {url}\n",
                filename = file.filename,
                url = if file.url.is_empty() {
                    "search results".to_string()
                } else {
                    file.url.clone()
                },
            ));
        }
    }

    if output.is_empty() {
        output.push_str("No cached files.");
    }

    output
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
                if let Some(ref calls) = msg.tool_calls {
                    for call in calls {
                        let name = call
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown");
                        let input = call
                            .get("input")
                            .map(|v| {
                                let s = v.to_string();
                                if s.len() > 200 {
                                    format!("{}...", &s[..200])
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

/// Clear all files in the `.web-cache/` directory.
pub fn clear_web_cache(workspace: &Path) -> Result<(), std::io::Error> {
    let cache_dir = workspace.join(".web-cache");
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

#[cfg(test)]
mod tests {
    use super::*;
    use surrealdb::sql::Datetime;
    use tempfile::TempDir;

    fn make_message(
        role: &str,
        content: &str,
        tool_calls: Option<Vec<serde_json::Value>>,
        tool_results: Option<Vec<serde_json::Value>>,
    ) -> MessageRecord {
        MessageRecord {
            id: Thing::from(("message", "test")),
            session: Thing::from(("session", "test")),
            role: role.to_string(),
            content: content.to_string(),
            tool_calls,
            tool_results,
            raw_output: None,
            created_at: Datetime::default(),
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
        let cache_dir = dir.path().join(".web-cache");
        std::fs::create_dir_all(&cache_dir).unwrap();

        std::fs::write(cache_dir.join("file1.md"), "content").unwrap();
        std::fs::write(cache_dir.join("file2.md"), "content").unwrap();

        assert!(cache_dir.join("file1.md").exists());

        clear_web_cache(dir.path()).unwrap();

        assert!(!cache_dir.join("file1.md").exists());
        assert!(!cache_dir.join("file2.md").exists());
        // Directory itself should still exist
        assert!(cache_dir.exists());
    }

    #[test]
    fn web_cache_clearing_missing_dir_is_ok() {
        let dir = TempDir::new().unwrap();
        assert!(clear_web_cache(dir.path()).is_ok());
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
        let cache_dir = dir.path().join(".web-cache");
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
        let classified = classify_web_cache(dir.path(), Some(findings), 500);

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
        let result = classify_web_cache(dir.path(), Some("no sources"), 500);
        assert!(result.is_empty());
    }

    #[test]
    fn format_classified_cache_sections() {
        let files = vec![
            ClassifiedCacheFile {
                filename: "cited.md".to_string(),
                url: "https://example.com/cited".to_string(),
                cited: true,
                preview: Some("# Title\nContent here".to_string()),
            },
            ClassifiedCacheFile {
                filename: "uncited.md".to_string(),
                url: "https://example.com/uncited".to_string(),
                cited: false,
                preview: None,
            },
        ];

        let output = format_classified_cache(&files);
        assert!(output.contains("### Cited in Agent Report"));
        assert!(output.contains("cited.md"));
        assert!(output.contains("### Uncited"));
        assert!(output.contains("uncited.md"));
    }
}
