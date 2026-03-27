use std::collections::VecDeque;
use std::fs;
#[cfg(feature = "live-tests")]
use std::io;
#[cfg(feature = "live-tests")]
use std::path::Path;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use ghost::config::{self, Config, SharedConfig};
use ghost::db::{self, GhostDb};
use ghost::knowledge::{NoteFrontMatter, serialize_note};
use ghost::providers::{
    ChatRequest, ChatResponse, ContentBlock, Provider, ProviderError, StopReason, ToolDefinition,
    Usage,
};
use ghost::tools::{Tool, ToolContext, ToolError};
use serde_json::json;
use tempfile::TempDir;

/// Wrap a `Config` into a `SharedConfig` for tests.
#[allow(dead_code, reason = "shared test helper not used by every test file")]
pub fn shared(config: &Config) -> SharedConfig {
    let (_tx, rx) = tokio::sync::watch::channel(Arc::new(config.clone()));
    rx
}

pub fn test_config() -> (Config, TempDir, TempDir) {
    let workspace = TempDir::new().expect("workspace tempdir");
    let config_dir = TempDir::new().expect("config tempdir");

    let config_file = config_dir.path().join("config.toml");
    fs::write(
        &config_file,
        format!(
            "workspace = \"{}\"\n\
\n\
[models]\n\
default = \"primary\"\n\
\n\
[models.primary]\n\
provider = \"openrouter\"\n\
model = \"anthropic/claude-sonnet-4-5-20250929\"\n\
context_window = 200000\n",
            workspace.path().display()
        ),
    )
    .expect("write config file");

    let config = config::load_from_dir(config_dir.path()).expect("load config");
    (config, workspace, config_dir)
}

pub fn test_workspace() -> (Config, TempDir, TempDir) {
    let (config, workspace, config_dir) = test_config();
    ghost::config_workspace::bootstrap_workspace(&config).expect("bootstrap workspace");
    (config, workspace, config_dir)
}

#[allow(dead_code, reason = "shared test helper not used by every test file")]
pub async fn test_database() -> (GhostDb, Config, TempDir, TempDir) {
    let (config, workspace, config_dir) = test_workspace();
    let db = db::connect(&config.workspace, config.embeddings.dimension)
        .await
        .expect("connect sqlite");
    (db, config, workspace, config_dir)
}

#[allow(dead_code, reason = "shared test helper not used by every test file")]
pub fn write_test_note(workspace: &std::path::Path, title: &str, body: &str) -> PathBuf {
    let front = NoteFrontMatter {
        title: title.to_string(),
        archetype: ghost::knowledge::Archetype::Entity,
        tags: vec![],
        parent: None,
        sources: vec![],
        trust: 5,
        written_at: "2026-01-01T00:00:00Z".into(),
        updated_at: None,
    };
    let content = serialize_note(&front, body).expect("serialize note");
    let slug = ghost::knowledge::slug_from_title(title);
    let path = workspace.join("notes").join(format!("{slug}.md"));
    fs::write(&path, content).expect("write test note");
    path
}

#[allow(dead_code, reason = "shared test helper not used by every test file")]
pub fn write_test_reference(
    workspace: &std::path::Path,
    topic: &str,
    filename: &str,
    content: &str,
) -> PathBuf {
    let dir = workspace.join("references").join(topic);
    fs::create_dir_all(&dir).expect("create reference dir");
    let path = dir.join(filename);
    fs::write(&path, content).expect("write test reference");
    path
}

// ---------------------------------------------------------------------------
// Live test infrastructure
// ---------------------------------------------------------------------------

/// Result from waiting for a background agent to complete.
#[cfg(feature = "live-tests")]
#[allow(
    dead_code,
    reason = "fields accessed by destructuring in live tests only; Rust doesn't see field reads through pattern bindings"
)]
pub struct AgentOutcome {
    /// GHOST's follow-up response after receiving agent findings.
    pub chat_result: ghost::chat::ChatResult,
    /// The agent's session ID (for querying its messages/metrics).
    pub agent_session: String,
    /// The raw findings text from the agent.
    pub findings: String,
}

/// Metrics collected from an agent session's web_fetch tool calls.
#[cfg(feature = "live-tests")]
#[allow(
    dead_code,
    reason = "fields accessed by destructuring in live tests only; Rust doesn't see field reads through pattern bindings"
)]
pub struct WebFetchMetrics {
    pub count: u32,
    pub urls: Vec<String>,
}

/// Environment for live e2e tests: fresh temp DB with real provider config.
///
/// On drop, snapshots the workspace and diagnostic log to
/// `e2e-output/<timestamp>_<test_name>/`.
#[cfg(feature = "live-tests")]
#[allow(
    dead_code,
    reason = "private fields are RAII guards or internal bookkeeping; public fields accessed selectively across test files"
)]
pub struct LiveTestEnv {
    pub db: GhostDb,
    pub config: Config,
    pub agent_runner: Arc<ghost::agents::AgentRunner>,
    workspace: TempDir,
    _config_dir: TempDir,
    test_name: String,
    prev_config_dir_env: Option<String>,
    prev_path_env: Option<String>,
    diagnostic_log: std::cell::RefCell<Vec<String>>,
    /// JSON diagnostic sections: keyed by label ("chat", "agent", "reflection").
    diagnostic_json: std::cell::RefCell<serde_json::Map<String, serde_json::Value>>,
    /// Per-session message count cursors for incremental logging.
    session_cursors: std::cell::RefCell<std::collections::HashMap<String, usize>>,
}

#[cfg(feature = "live-tests")]
#[allow(
    dead_code,
    reason = "impl methods are helpers selectively used across different live test files"
)]
impl LiveTestEnv {
    /// Absolute path to this test environment's workspace directory.
    pub fn workspace_path(&self) -> &Path {
        self.workspace.path()
    }

    /// Boot the real daemon with this test's config. Discord is skipped
    /// because no DISCORD_BOT_TOKEN is set in the test environment.
    pub async fn boot_daemon(&self) -> ghost::daemon::DaemonHandle {
        // Ensure no Discord token leaks from the host env
        // SAFETY: called during single-threaded test setup before daemon boot
        unsafe {
            std::env::remove_var("DISCORD_BOT_TOKEN");
        }
        ghost::daemon::boot_with_config(self.config.clone())
            .await
            .expect("daemon boot failed")
    }

    /// Write a full workspace snapshot as a tar.zst archive.
    pub fn write_workspace_archive(&self, dest: &Path) {
        write_workspace_archive(self.workspace.path(), dest)
            .unwrap_or_else(|e| panic!("write workspace archive at {}: {e}", dest.display()));
    }

    /// Collect messages from a session into a JSON array of simplified
    /// message objects for the diagnostic log. Each message is
    /// `{ "role", "content"?, "tool_calls"?, "tool_results"? }`.
    pub async fn collect_session_json(&self, session_id: &str) -> Vec<serde_json::Value> {
        let messages = ghost::db::sessions::list_messages_by_session(&self.db, session_id)
            .await
            .unwrap_or_default();

        messages
            .iter()
            .map(|msg| {
                let mut obj = serde_json::Map::new();
                obj.insert("role".into(), json!(msg.role));

                if !msg.content.trim().is_empty() {
                    obj.insert("content".into(), json!(truncate_str(&msg.content, 3000)));
                }

                if let Some(calls) = msg.tool_calls_parsed() {
                    let simplified: Vec<serde_json::Value> = calls
                        .iter()
                        .map(|c| {
                            json!({
                                "name": c.get("name").and_then(|v| v.as_str()).unwrap_or("?"),
                                "id": c.get("id").and_then(|v| v.as_str()).unwrap_or("?"),
                                "input": c.get("input").cloned().unwrap_or(json!(null)),
                            })
                        })
                        .collect();
                    obj.insert("tool_calls".into(), json!(simplified));
                }

                if let Some(results) = msg.tool_results_parsed() {
                    let simplified: Vec<serde_json::Value> = results
                        .iter()
                        .map(|r| {
                            json!({
                                "tool_use_id": r.get("tool_use_id")
                                    .and_then(|v| v.as_str()).unwrap_or("?"),
                                "is_error": r.get("is_error")
                                    .and_then(serde_json::Value::as_bool).unwrap_or(false),
                                "content": truncate_str(
                                    r.get("content")
                                        .and_then(|v| v.as_str()).unwrap_or(""),
                                    2000,
                                ),
                            })
                        })
                        .collect();
                    obj.insert("tool_results".into(), json!(simplified));
                }

                if let Some(raw) = msg.raw_output_parsed() {
                    let simplified: Vec<serde_json::Value> = raw
                        .iter()
                        .map(|item| {
                            let original_type = item
                                .get("original_type")
                                .and_then(|v| v.as_str())
                                .unwrap_or("unknown");
                            let summary_arr = item
                                .get("value")
                                .and_then(|v| v.get("summary"))
                                .and_then(|v| v.as_array());
                            let summary = match summary_arr {
                                Some(arr) => arr
                                    .iter()
                                    .filter_map(|s| s.get("text").and_then(|v| v.as_str()))
                                    .collect::<Vec<_>>()
                                    .join(" "),
                                None => String::new(),
                            };
                            json!({
                                "original_type": original_type,
                                "summary": truncate_str(&summary, 1200),
                            })
                        })
                        .collect();
                    if !simplified.is_empty() {
                        obj.insert("raw_output".into(), json!(simplified));
                    }
                }

                serde_json::Value::Object(obj)
            })
            .collect()
    }

    /// Add a custom note to the diagnostic log.
    pub fn log(&self, msg: impl std::fmt::Display) {
        self.diagnostic_log.borrow_mut().push(format!("# {msg}"));
    }

    /// Collect a session's messages and store them in the JSON diagnostic
    /// output under the given label (e.g. "chat", "agent", "reflection").
    /// Records a cursor so the next call to `log_session_json_since` on
    /// the same session only includes new messages.
    pub async fn log_session_json(&self, label: &str, session_id: &str) {
        let messages = self.collect_session_json(session_id).await;
        let count = messages.len();
        self.diagnostic_json
            .borrow_mut()
            .insert(label.to_string(), json!(messages));
        self.session_cursors
            .borrow_mut()
            .insert(session_id.to_string(), count);
    }

    /// Like `log_session_json`, but only includes messages added since the
    /// last `log_session_json` call for this session. Avoids duplicating
    /// already-logged content.
    pub async fn log_session_json_since(&self, label: &str, session_id: &str) {
        let all_messages = self.collect_session_json(session_id).await;
        let cursor = self
            .session_cursors
            .borrow()
            .get(session_id)
            .copied()
            .unwrap_or(0);
        let new_messages: Vec<_> = all_messages.into_iter().skip(cursor).collect();
        let count_after = cursor + new_messages.len();
        self.diagnostic_json
            .borrow_mut()
            .insert(label.to_string(), json!(new_messages));
        self.session_cursors
            .borrow_mut()
            .insert(session_id.to_string(), count_after);
    }

    // -----------------------------------------------------------------
    // Session helpers
    // -----------------------------------------------------------------

    /// Create a bare session.
    pub async fn create_session(&self) -> String {
        ghost::db::sessions::create_session(&self.db)
            .await
            .expect("create session")
    }

    /// Create a session with pre-filled messages.
    pub async fn session_with_messages(&self, messages: &[(&str, &str)]) -> String {
        let session_id = self.create_session().await;
        for (role, content) in messages {
            ghost::db::sessions::create_message(&self.db, &session_id, role, content)
                .await
                .expect("create message");
        }
        session_id
    }

    /// Replay a diagnostic-format JSON transcript into a fresh session.
    ///
    /// Each entry has `role`, optional `content`, optional `tool_calls`,
    /// optional `tool_results`. Returns the session ID with all messages stored.
    pub async fn session_from_transcript(&self, transcript: &[serde_json::Value]) -> String {
        let session_id = self.create_session().await;
        for msg in transcript {
            let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("user");
            let content = msg.get("content").and_then(|v| v.as_str()).unwrap_or("");
            let tool_calls: Option<Vec<serde_json::Value>> =
                msg.get("tool_calls").and_then(|v| v.as_array()).cloned();
            let tool_results: Option<Vec<serde_json::Value>> =
                msg.get("tool_results").and_then(|v| v.as_array()).cloned();
            ghost::db::sessions::create_message_with_metadata(
                &self.db,
                &session_id,
                role,
                content,
                &ghost::db::sessions::MessagePayload {
                    tool_calls,
                    tool_results,
                    ..Default::default()
                },
            )
            .await
            .expect("replay message");
        }
        session_id
    }

    /// Copy a fixture directory into the workspace's `.cache/{session_id}/`.
    pub fn install_web_cache_fixture(&self, session_id: &str, fixture_dir: &std::path::Path) {
        let dest = self.workspace.path().join(".cache").join(session_id);
        fs::create_dir_all(&dest).expect("create .cache dir");
        for entry in fs::read_dir(fixture_dir).expect("read fixture dir") {
            let entry = entry.expect("read dir entry");
            if entry.file_type().expect("file type").is_file() {
                fs::copy(entry.path(), dest.join(entry.file_name())).expect("copy fixture file");
            }
        }
    }

    // -----------------------------------------------------------------
    // Chat helpers
    // -----------------------------------------------------------------

    /// SessionChat with real provider + chat tools + agent runner.
    pub fn chat(&self) -> ghost::chat::SessionChat {
        ghost::chat::SessionChat::from_config(self.db.clone(), shared(&self.config))
            .expect("build session chat")
            .with_agent_runner(Arc::clone(&self.agent_runner))
    }

    /// SessionChat wired with an event channel + spawned event handler,
    /// mirroring the production daemon's wiring. Returns the chat handle
    /// and the handler's join handle.
    pub fn chat_with_event_handler(
        &self,
    ) -> (Arc<ghost::chat::SessionChat>, tokio::task::JoinHandle<()>) {
        let (event_tx, event_rx) = ghost::events::channel();
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        // Keep shutdown_tx alive by leaking — test cleanup doesn't need graceful shutdown.
        std::mem::forget(shutdown_tx);

        let session_chat = Arc::new(
            ghost::chat::SessionChat::from_config(self.db.clone(), shared(&self.config))
                .expect("build session chat")
                .with_agent_runner(Arc::clone(&self.agent_runner))
                .with_event_sender(event_tx),
        );

        let handler_handle = ghost::daemon::event_handler::spawn_event_handler(
            event_rx,
            Arc::clone(&session_chat),
            None, // no Discord in tests
            self.db.clone(),
            shutdown_rx,
        );

        (session_chat, handler_handle)
    }

    /// Poll DB for a final assistant message (no tool_calls) beyond
    /// `since_message_count`. Used to detect the continuation response
    /// triggered by the completion watcher.
    pub async fn wait_for_continuation_response(
        &self,
        session_id: &str,
        since_message_count: usize,
        timeout_secs: u64,
    ) -> Option<String> {
        use std::time::{Duration, Instant};

        let deadline = Instant::now() + Duration::from_secs(timeout_secs);
        let poll_interval = Duration::from_secs(5);

        loop {
            let messages = ghost::db::sessions::list_messages_by_session(&self.db, session_id)
                .await
                .unwrap_or_default();

            // Look at messages beyond the cursor
            for msg in messages.iter().skip(since_message_count) {
                if msg.role == "assistant"
                    && !msg.content.trim().is_empty()
                    && msg.tool_calls_parsed().is_none_or(|tc| tc.is_empty())
                {
                    return Some(msg.content.clone());
                }
            }

            if Instant::now() >= deadline {
                self.log(format!(
                    "TIMEOUT: no continuation response after {timeout_secs}s"
                ));
                return None;
            }

            tokio::time::sleep(poll_interval).await;
        }
    }

    // -----------------------------------------------------------------
    // Job runners
    // -----------------------------------------------------------------

    /// Run reflection on a session via the agent runner.
    ///
    /// Run a reflection agent on a chat session.
    ///
    /// The agent's Lua `build()` assembles its own user message from
    /// `ctx` bindings (transcript, diary, etc.). Post-processing
    /// (web cache curation) happens in the agent's `post_completion`.
    pub async fn run_reflection(
        &self,
        session_id: &str,
        agent_name: &str,
    ) -> (String, ghost::chat::RunMetadata) {
        let result = self
            .agent_runner
            .run(agent_name, "Begin reflection.", Some(session_id))
            .await
            .expect("reflection run");

        (result.findings, result.metadata)
    }

    /// Structured reflection: run the deep-research-reflection agent with
    /// pre-extracted structured report data (from `report_findings` tool).
    /// Avoids loading the full research conversation — the agent works
    /// entirely from the structured JSON.
    pub async fn run_structured_reflection(
        &self,
        agent_session_id: &str,
        report_data_json: &str,
    ) -> (String, ghost::chat::RunMetadata) {
        // Classify web cache BEFORE reflection (for post-processing).
        // Use the report field from structured data for citation matching.
        let report_text: Option<String> =
            serde_json::from_str::<serde_json::Value>(report_data_json)
                .ok()
                .and_then(|v| v.get("report").and_then(|r| r.as_str()).map(String::from));

        let classified = ghost::web::classify_web_cache(
            &self.config.workspace,
            agent_session_id,
            report_text.as_deref(),
            1000,
        );

        // Run deep-research-reflection with structured data as args
        let args = std::collections::HashMap::from([
            ("report_data".to_string(), report_data_json.to_string()),
            ("session_id".to_string(), agent_session_id.to_string()),
        ]);
        let result = Box::pin(self.agent_runner.run_with_args(
            "deep-research-reflection",
            args,
            Some(agent_session_id),
        ))
        .await
        .expect("structured reflection run_with_args");
        let (findings, metadata) = (result.findings, result.metadata);

        // Post-processing: deterministic reference curation (matches production)
        let curation =
            ghost::web::curate_references(&self.config.workspace, agent_session_id, &classified);
        self.log(format!(
            "curate_references: {} moved, {} deleted",
            curation.moved, curation.deleted,
        ));

        // Create cited edges (note → reference) in the knowledge graph
        let cited =
            ghost::web::link_cited_edges(&self.db, &self.config.workspace, &classified).await;
        self.log(format!("link_cited_edges: {cited} created"));

        (findings, metadata)
    }

    // -----------------------------------------------------------------
    // Agent helpers
    // -----------------------------------------------------------------

    /// Poll for all background agents to complete, inject their findings
    /// into the parent session, trigger a follow-up chat turn, and return
    /// the final response. Mirrors what the agent watcher does in the daemon.
    ///
    /// Times out after `timeout_secs` seconds.
    pub async fn wait_for_agents(
        &self,
        session_id: &str,
        timeout_secs: u64,
    ) -> Option<AgentOutcome> {
        use std::time::{Duration, Instant};

        let deadline = Instant::now() + Duration::from_secs(timeout_secs);
        let poll_interval = Duration::from_secs(3);

        loop {
            if Instant::now() >= deadline {
                self.log("TIMEOUT: agent did not complete in time");
                return None;
            }

            let agent_ids = self.agent_runner.list_agent_ids().await;
            if agent_ids.is_empty() {
                self.log("no agents found (model may not have spawned one)");
                return None;
            }

            for agent_id in &agent_ids {
                let Ok(status) = self.agent_runner.status(agent_id).await else {
                    continue;
                };
                if status.status != "completed" {
                    continue;
                }

                let findings = status
                    .findings
                    .as_deref()
                    .unwrap_or("Agent completed without producing findings.");

                self.log(format!(
                    "agent '{}' completed ({} messages)",
                    status.agent_name, status.message_count
                ));

                // Extract bare session ID from agent_id
                let agent_session_id = agent_id
                    .split_once(':')
                    .map(|(_, id)| id.to_string())
                    .unwrap_or_else(|| agent_id.clone());

                self.log_session_json("agent", &agent_session_id).await;

                // finish_background already injected findings into parent.
                // Trigger follow-up chat turn on parent session.
                let chat = self.chat();
                let trigger = "[system] Research agent completed.";
                match chat.chat(session_id, trigger, None, None).await {
                    Ok((result, _metadata)) => {
                        return Some(AgentOutcome {
                            chat_result: result,
                            agent_session: agent_session_id,
                            findings: findings.to_string(),
                        });
                    }
                    Err(e) => {
                        self.log(format!("ERROR: follow-up chat turn failed: {e}"));
                        return None;
                    }
                }
            }

            tokio::time::sleep(poll_interval).await;
        }
    }

    // -----------------------------------------------------------------
    // Polling helpers
    // -----------------------------------------------------------------

    /// Poll the database for a system message containing `pattern`.
    ///
    /// Checks every 5 seconds. Returns the full message content on match,
    /// or `None` if `timeout_secs` elapses without a match.
    pub async fn wait_for_system_message(
        &self,
        session_id: &str,
        pattern: &str,
        timeout_secs: u64,
    ) -> Option<String> {
        use std::time::{Duration, Instant};

        let deadline = Instant::now() + Duration::from_secs(timeout_secs);
        let poll_interval = Duration::from_secs(5);

        loop {
            let messages = ghost::db::sessions::list_messages_by_session(&self.db, session_id)
                .await
                .unwrap_or_default();

            for msg in &messages {
                if msg.role == "system" && msg.content.contains(pattern) {
                    return Some(msg.content.clone());
                }
            }

            if Instant::now() >= deadline {
                self.log(format!(
                    "TIMEOUT: no system message matching '{pattern}' after {timeout_secs}s"
                ));
                return None;
            }

            tokio::time::sleep(poll_interval).await;
        }
    }

    // -----------------------------------------------------------------
    // Assertion helpers
    // -----------------------------------------------------------------

    /// Collect all tool call names from a session's messages, in order.
    pub async fn collect_tool_calls(&self, session_id: &str) -> Vec<String> {
        let messages = ghost::db::sessions::list_messages_by_session(&self.db, session_id)
            .await
            .expect("list session messages for tool calls");

        messages
            .iter()
            .filter_map(ghost::db::sessions::MessageRecord::tool_calls_parsed)
            .flatten()
            .filter_map(|call| call.get("name").and_then(|v| v.as_str()).map(String::from))
            .collect()
    }

    /// Stop all running agents immediately and return how many were stopped.
    pub async fn stop_all_agents(&self) -> usize {
        let ids = self.agent_runner.list_agent_ids().await;
        let count = ids.len();
        for id in &ids {
            let _ = self.agent_runner.stop(id).await;
        }
        count
    }

    /// Collect web_fetch metrics from an agent session's messages.
    pub async fn collect_web_fetch_metrics(&self, session_id: &str) -> WebFetchMetrics {
        let messages = ghost::db::sessions::list_messages_by_session(&self.db, session_id)
            .await
            .expect("list session messages for metrics");

        let mut count = 0u32;
        let mut urls = Vec::new();

        let all_calls: Vec<_> = messages
            .iter()
            .filter_map(ghost::db::sessions::MessageRecord::tool_calls_parsed)
            .flatten()
            .collect();
        for call in &all_calls {
            let name = call
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            if name != "web_fetch" {
                continue;
            }
            count += 1;
            if let Some(url) = call
                .get("input")
                .and_then(|v| v.get("url"))
                .and_then(|v| v.as_str())
            {
                urls.push(url.to_string());
            }
        }

        WebFetchMetrics { count, urls }
    }

    /// Assert that research findings meet quality bar: substantial text,
    /// minimum web_fetch count, expected domains fetched, expected keywords
    /// present in findings. Panics with descriptive messages on failure.
    pub fn assert_research_quality(
        &self,
        findings: &str,
        metrics: &WebFetchMetrics,
        expected_domains: &[&str],
        expected_keywords: &[&str],
    ) {
        assert!(
            findings.len() > 200,
            "expected substantial findings (>200 chars), got {} chars",
            findings.len()
        );

        self.log(format!("web_fetch calls: {}", metrics.count));
        self.log(format!("web_fetch urls: {:?}", metrics.urls));

        assert!(
            metrics.count >= 5,
            "expected >= 5 web_fetch calls, got {}",
            metrics.count
        );

        let matched: Vec<&str> = expected_domains
            .iter()
            .copied()
            .filter(|domain| metrics.urls.iter().any(|url| url.contains(domain)))
            .collect();
        assert!(
            !matched.is_empty(),
            "expected at least one fetch from any of {expected_domains:?}, \
             but none were found in fetched URLs"
        );
        self.log(format!(
            "specialist domains matched: {matched:?} (of {expected_domains:?})"
        ));

        let findings_lower = findings.to_lowercase();
        for keyword in expected_keywords {
            assert!(
                findings_lower.contains(&keyword.to_lowercase()),
                "expected '{keyword}' in findings (case-insensitive)"
            );
        }
    }

    /// Stop a running agent and reset its session to only the initial user
    /// message (the spawning prompt). This gives a clean DB state for
    /// snapshotting — no mid-flight tool calls or partial results.
    pub async fn stop_and_reset_agent(&self, agent_id: &str) {
        // Stop the agent (cancels the background task)
        let _ = self.agent_runner.stop(agent_id).await;

        // Parse bare session ID from agent_id (e.g. "session:abc" → "abc")
        let session_id = agent_id
            .split_once(':')
            .map(|(_, id)| id)
            .unwrap_or(agent_id);

        // Keep only the first message (the initial user prompt).
        // Delete everything else — assistant tool calls, tool results, etc.
        sqlx::query(
            "DELETE FROM message WHERE session_id = ?1
             AND id != (
                 SELECT id FROM message
                 WHERE session_id = ?1
                 ORDER BY created_at ASC
                 LIMIT 1
             )",
        )
        .bind(session_id)
        .execute(&self.db)
        .await
        .unwrap_or_else(|e| panic!("reset agent session {session_id}: {e}"));

        // Also clear any TODO state the agent may have created
        sqlx::query("UPDATE session SET todo_list = NULL WHERE id = ?1")
            .bind(session_id)
            .execute(&self.db)
            .await
            .unwrap_or_else(|e| panic!("clear agent todo for {session_id}: {e}"));
    }

    /// Check if a file exists under the workspace.
    pub fn workspace_file_exists(&self, relative_path: &str) -> bool {
        self.workspace.path().join(relative_path).exists()
    }

    /// Read a workspace file's content.
    pub fn read_workspace_file(&self, relative_path: &str) -> Option<String> {
        fs::read_to_string(self.workspace.path().join(relative_path)).ok()
    }

    /// List all notes in notes/.
    pub fn list_notes(&self) -> Vec<PathBuf> {
        list_files_in(self.workspace.path(), "notes")
    }

    /// List all references in references/.
    pub fn list_references(&self) -> Vec<PathBuf> {
        list_files_in(self.workspace.path(), "references")
    }

    /// Load a Lua agent config from the temp workspace (populated by
    /// `bundled::install_all()` during bootstrap).
    pub fn load_agent(&self, name: &str) -> ghost::scripting::types::AgentConfig {
        ghost::agents::load_agent(&self.config.workspace, name)
            .unwrap_or_else(|e| panic!("load agent '{name}': {e}"))
    }

    /// Recursively search for any file under a workspace subdirectory
    /// whose content contains a string.
    pub fn find_file_containing(&self, dir: &str, needle: &str) -> bool {
        find_file_containing_recursive(&self.workspace.path().join(dir), needle)
    }

    /// Assert at least one note file contains one of the needles.
    ///
    /// Panics with a descriptive message including `description` if no
    /// note matches any needle.
    pub fn assert_notes_contain_any(&self, needles: &[&str], description: &str) {
        let found = needles
            .iter()
            .any(|needle| self.find_file_containing("notes", needle));
        assert!(
            found,
            "expected a note containing one of {needles:?} ({description})"
        );
    }

    /// Assert a diary entry exists for today; return its contents.
    ///
    /// Looks for `diary/{YYYY-MM-DD}.md` in the workspace. Panics if
    /// the file doesn't exist.
    pub fn assert_diary_exists(&self) -> String {
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let diary_path = format!("diary/{today}.md");
        let content = self
            .read_workspace_file(&diary_path)
            .unwrap_or_else(|| panic!("expected diary entry at {diary_path}"));
        assert!(
            !content.trim().is_empty(),
            "diary entry at {diary_path} exists but is empty"
        );
        content
    }
}

#[cfg(feature = "live-tests")]
impl Drop for LiveTestEnv {
    fn drop(&mut self) {
        // Snapshot workspace for human validation
        let timestamp = chrono::Utc::now().format("%Y-%m-%dT%H-%M-%S");
        let dest = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("e2e-output")
            .join(format!("{timestamp}_{}", self.test_name));
        if let Err(e) = copy_dir_all(self.workspace.path(), &dest) {
            eprintln!("warning: failed to snapshot workspace: {e}");
        }

        // Write JSON diagnostic (structured session data) — file only, not stderr
        let json_map = self.diagnostic_json.borrow();
        if !json_map.is_empty() {
            let json_value = serde_json::Value::Object(json_map.clone());
            let json_str = serde_json::to_string_pretty(&json_value).unwrap_or_default();
            let json_path = dest.join("diagnostic.json");
            if let Err(e) = fs::write(&json_path, &json_str) {
                eprintln!("warning: failed to write diagnostic json: {e}");
            }
        }

        // Write text log (freeform notes) — file only, not stderr
        let log = self.diagnostic_log.borrow();
        if !log.is_empty() {
            let log_content = log.join("\n");
            let log_path = dest.join("diagnostic.log");
            if let Err(e) = fs::write(&log_path, &log_content) {
                eprintln!("warning: failed to write diagnostic log: {e}");
            }
        }

        // Count raw request files saved by debug.save_requests
        let requests_dir = dest.join("debug").join("requests");
        let request_count = requests_dir
            .read_dir()
            .map(|rd| rd.filter_map(Result::ok).count())
            .unwrap_or(0);

        // One-line summary with paths to all artifacts
        eprintln!(
            "e2e snapshot: {} (diagnostic.json, diagnostic.log, {request_count} raw requests in debug/requests/)",
            dest.display(),
        );

        // Restore env vars
        match &self.prev_config_dir_env {
            Some(val) => {
                // SAFETY: called during test teardown; concurrent env mutation is accepted in tests
                unsafe { std::env::set_var(ghost::config::CONFIG_DIR_ENV, val) };
            }
            None => {
                // SAFETY: called during test teardown; concurrent env mutation is accepted in tests
                unsafe { std::env::remove_var(ghost::config::CONFIG_DIR_ENV) };
            }
        }
        if let Some(val) = &self.prev_path_env {
            // SAFETY: same as above — restoring PATH during test teardown
            unsafe { std::env::set_var("PATH", val) };
        }
    }
}

/// Create a live test environment: real provider from `~/.config/ghost/`,
/// fresh temp workspace + database, `GHOST_CONFIG_DIR` set so spawned
/// `ghost` subprocesses use the temp workspace.
#[cfg(feature = "live-tests")]
#[allow(
    dead_code,
    reason = "shared live-test helper not used by every test file that includes common.rs"
)]
pub async fn live_test_database(test_name: &str) -> LiveTestEnv {
    live_test_database_from_snapshot(test_name, None).await
}

/// Create a live test environment restored from a tar.zst workspace snapshot.
///
/// The archive is extracted into the temp workspace after bootstrap and before
/// database connection, so SQLite state is restored cleanly.
#[cfg(feature = "live-tests")]
#[allow(
    dead_code,
    reason = "shared live-test helper not used by every test file that includes common.rs"
)]
#[allow(
    clippy::unwrap_used,
    reason = "test helper — panicking on None is the desired behavior"
)]
pub async fn live_test_database_from_snapshot(
    test_name: &str,
    snapshot: Option<&Path>,
) -> LiveTestEnv {
    let _ = ghost::observability::init_for_live_tests();

    // Save current env state (before we change anything)
    let prev_config_dir = std::env::var(ghost::config::CONFIG_DIR_ENV).ok();
    let prev_path = std::env::var("PATH").ok();

    // Find the real config dir
    let real_config_dir = std::env::var_os(ghost::config::CONFIG_DIR_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = std::env::var("HOME").expect("HOME env var");
            PathBuf::from(home).join(".config/ghost")
        });

    // Create temp workspace + config dir
    let workspace = TempDir::new().expect("workspace tempdir");
    let config_dir = TempDir::new().expect("config tempdir");

    // Read real config.toml, replace workspace path only
    let raw_toml = fs::read_to_string(real_config_dir.join("config.toml"))
        .expect("read real config.toml — is ~/.config/ghost/config.toml present?");
    let mut toml_value: toml::Value = toml::from_str(&raw_toml).expect("parse real config.toml");
    toml_value.as_table_mut().unwrap().insert(
        "workspace".to_string(),
        toml::Value::String(workspace.path().display().to_string()),
    );
    if let Ok(model_alias) = std::env::var("GHOST_E2E_MODEL")
        && let Some(models) = toml_value
            .get_mut("models")
            .and_then(toml::Value::as_table_mut)
    {
        models.insert("default".to_string(), toml::Value::String(model_alias));
    }
    let modified_toml = toml::to_string_pretty(&toml_value).expect("serialize config");
    fs::write(config_dir.path().join("config.toml"), &modified_toml)
        .expect("write temp config.toml");

    // Copy tokens/ and .env from real config dir (OAuth tokens, secrets)
    let tokens_src = real_config_dir.join("tokens");
    if tokens_src.exists() {
        copy_dir_all(&tokens_src, &config_dir.path().join("tokens")).expect("copy tokens dir");
    }
    let env_src = real_config_dir.join(".env");
    if env_src.exists() {
        fs::copy(&env_src, config_dir.path().join(".env")).expect("copy .env");
    }

    // Set env vars so both us and spawned `ghost` processes use the temp config
    // SAFETY: called during test setup; env mutation is acceptable in live test helpers
    unsafe {
        std::env::set_var(ghost::config::CONFIG_DIR_ENV, config_dir.path());

        // Add target/debug to PATH so `ghost web fetch` works in subprocess
        let target_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/debug");
        let path = std::env::var("PATH").unwrap_or_default();
        std::env::set_var("PATH", format!("{}:{}", target_dir.display(), path));
    }

    // Load config from temp dir + bootstrap + connect
    let mut config = config::load_from_dir(config_dir.path()).expect("load config from temp dir");
    config.debug.save_requests = true;
    config.install_bundled_docs = false;
    ghost::config_workspace::bootstrap_workspace(&config).expect("bootstrap temp workspace");
    if let Some(snapshot_path) = snapshot {
        restore_workspace_archive(&config.workspace, snapshot_path).unwrap_or_else(|e| {
            panic!(
                "restore workspace snapshot from {}: {e}",
                snapshot_path.display()
            )
        });
        // Bundled files always overwrite, so the restored snapshot gets
        // the binary's current prompts, not stale fixture versions.
        ghost::bundled::install_all(&config.workspace)
            .expect("install bundled files after snapshot restore");
    }
    let db = db::connect(&config.workspace, config.embeddings.dimension)
        .await
        .expect("connect to fresh temp database");

    let agent_runner = Arc::new(ghost::agents::AgentRunner::new(
        db.clone(),
        shared(&config),
        None,
    ));

    LiveTestEnv {
        db,
        config,
        agent_runner,
        workspace,
        _config_dir: config_dir,
        test_name: test_name.to_string(),
        prev_config_dir_env: prev_config_dir,
        prev_path_env: prev_path,
        diagnostic_log: std::cell::RefCell::new(Vec::new()),
        diagnostic_json: std::cell::RefCell::new(serde_json::Map::new()),
        session_cursors: std::cell::RefCell::new(std::collections::HashMap::new()),
    }
}

#[cfg(feature = "live-tests")]
fn write_workspace_archive(workspace: &Path, dest: &Path) -> io::Result<()> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = fs::File::create(dest)?;
    let encoder = zstd::stream::write::Encoder::new(file, 3)?;
    let mut tar_builder = tar::Builder::new(encoder.auto_finish());
    tar_builder.append_dir_all(".", workspace)?;
    tar_builder.finish()?;
    Ok(())
}

#[cfg(feature = "live-tests")]
fn restore_workspace_archive(workspace: &Path, snapshot: &Path) -> io::Result<()> {
    clear_dir_contents(workspace)?;
    let file = fs::File::open(snapshot)?;
    let decoder = zstd::stream::read::Decoder::new(file)?;
    let mut archive = tar::Archive::new(decoder);
    archive.unpack(workspace)?;
    Ok(())
}

#[cfg(feature = "live-tests")]
fn clear_dir_contents(dir: &Path) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            fs::remove_dir_all(path)?;
        } else {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}

#[cfg(feature = "live-tests")]
fn list_files_in(workspace: &std::path::Path, subdir: &str) -> Vec<PathBuf> {
    let dir = workspace.join(subdir);
    let mut files = Vec::new();
    collect_files_recursive(&dir, &mut files);
    files
}

#[cfg(feature = "live-tests")]
fn collect_files_recursive(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files_recursive(&path, out);
        } else if path.is_file() {
            out.push(path);
        }
    }
}

#[cfg(feature = "live-tests")]
fn find_file_containing_recursive(dir: &std::path::Path, needle: &str) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if find_file_containing_recursive(&path, needle) {
                return true;
            }
        } else if path.is_file()
            && let Ok(content) = fs::read_to_string(&path)
            && content.contains(needle)
        {
            return true;
        }
    }
    false
}

#[cfg(feature = "live-tests")]
fn truncate_str(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        // Find a valid UTF-8 char boundary at or before `max`
        let boundary = s.floor_char_boundary(max);
        format!("{}…[{}b total]", &s[..boundary], s.len())
    }
}

#[cfg(feature = "live-tests")]
fn copy_dir_all(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let dest_path = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_all(&entry.path(), &dest_path)?;
        } else {
            fs::copy(entry.path(), dest_path)?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Mock provider + test tool helpers (used by non-live tests)
// ---------------------------------------------------------------------------

#[derive(Debug)]
#[allow(
    dead_code,
    reason = "private fields accessed only through the Provider trait impl and requests() accessor"
)]
pub struct MockProvider {
    responses: Arc<Mutex<VecDeque<ChatResponse>>>,
    requests: Arc<Mutex<Vec<ChatRequest>>>,
}

#[allow(
    dead_code,
    reason = "helper methods used selectively across test files that include common.rs"
)]
impl MockProvider {
    pub fn new(responses: Vec<ChatResponse>) -> Self {
        Self {
            responses: Arc::new(Mutex::new(VecDeque::from(responses))),
            requests: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn requests(&self) -> Arc<Mutex<Vec<ChatRequest>>> {
        Arc::clone(&self.requests)
    }
}

#[async_trait]
impl Provider for MockProvider {
    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, ProviderError> {
        self.requests.lock().expect("lock requests").push(request);
        self.responses
            .lock()
            .expect("lock responses")
            .pop_front()
            .ok_or_else(|| ProviderError::InvalidResponse("no mock response remaining".to_string()))
    }

    fn name(&self) -> &str {
        "mock"
    }
}

#[derive(Debug)]
#[allow(
    dead_code,
    reason = "test-only tool type; not referenced by every test file that includes common.rs"
)]
pub struct EchoTool;

#[async_trait]
impl Tool for EchoTool {
    fn name(&self) -> &str {
        "echo_tool"
    }

    fn schema(&self) -> ToolDefinition {
        ToolDefinition {
            name: "echo_tool".to_string(),
            description: "echoes input".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": { "text": { "type": "string" } },
                "required": ["text"]
            }),
        }
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &ToolContext,
    ) -> Result<ghost::tools::ToolOutput, ToolError> {
        let text = params
            .get("text")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        Ok(ghost::tools::ToolOutput::text(format!("echo:{text}")))
    }
}

#[allow(
    dead_code,
    reason = "shared test helper not used by every test file that includes common.rs"
)]
pub fn response(content: Vec<ContentBlock>, stop_reason: StopReason) -> ChatResponse {
    ChatResponse {
        content,
        usage: Usage::default(),
        stop_reason,
        model: "mock-model".to_string(),
        response_id: None,
        turn_state: None,
    }
}

/// Build a mock response that ends the turn with a plain text message.
#[allow(
    dead_code,
    reason = "shared test helper not used by every test file that includes common.rs"
)]
pub fn respond_response(message: &str, _citations: Vec<serde_json::Value>) -> ChatResponse {
    response(
        vec![ContentBlock::Text {
            text: message.to_string(),
        }],
        StopReason::EndTurn,
    )
}
