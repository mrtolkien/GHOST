use std::collections::VecDeque;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use ghost::config::{self, Config};
use ghost::db::{self, GhostDb};
use ghost::knowledge::{NoteFrontMatter, serialize_note};
use ghost::providers::{
    ChatRequest, ChatResponse, ContentBlock, Provider, ProviderError, StopReason, ToolDefinition,
    Usage,
};
use ghost::tools::{Tool, ToolContext, ToolError};
use serde_json::json;
#[cfg(feature = "live-tests")]
use surrealdb::sql::Thing;
use tempfile::TempDir;

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

#[allow(dead_code)]
pub async fn test_database() -> (GhostDb, Config, TempDir, TempDir) {
    let (config, workspace, config_dir) = test_workspace();
    let db = db::connect(&config.workspace)
        .await
        .expect("connect surrealdb");
    (db, config, workspace, config_dir)
}

#[allow(dead_code)]
pub fn write_test_note(workspace: &std::path::Path, title: &str, body: &str) -> PathBuf {
    let front = NoteFrontMatter {
        title: title.to_string(),
        archetype: None,
        tags: vec![],
        trust: 5,
    };
    let content = serialize_note(&front, body).expect("serialize note");
    let slug = ghost::knowledge::slug_from_title(title);
    let path = workspace.join("notes").join(format!("{slug}.md"));
    fs::write(&path, content).expect("write test note");
    path
}

#[allow(dead_code)]
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

/// Environment for live e2e tests: fresh temp DB with real provider config.
///
/// On drop, snapshots the workspace and diagnostic log to
/// `e2e-output/<timestamp>_<test_name>/`.
#[cfg(feature = "live-tests")]
#[allow(dead_code)]
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
}

#[cfg(feature = "live-tests")]
#[allow(dead_code)]
impl LiveTestEnv {
    /// Dump all messages from a session into the diagnostic log.
    ///
    /// Call this after each `chat()` or `chat_job()` to capture the full
    /// conversation for post-mortem analysis. Produces a detailed,
    /// human-readable transcript with numbered turns, full tool call
    /// inputs/outputs, and timestamps.
    pub async fn log_session(&self, label: &str, session_id: &surrealdb::sql::Thing) {
        let messages = ghost::db::sessions::list_messages_by_session(&self.db, session_id)
            .await
            .unwrap_or_default();

        let mut log = self.diagnostic_log.borrow_mut();
        let sep = "=".repeat(72);
        log.push(format!("\n{sep}"));
        log.push(format!("  {label} (session:{session_id})"));
        log.push(format!(
            "  messages: {}  |  started: {}",
            messages.len(),
            messages
                .first()
                .map(|m| m.created_at.to_string())
                .unwrap_or_else(|| "—".to_string()),
        ));
        log.push(sep.clone());

        let mut turn = 0usize;
        let mut tool_call_count = 0usize;

        for msg in &messages {
            turn += 1;
            let ts = &msg.created_at;

            match msg.role.as_str() {
                // Tool results are stored as "user" messages with tool_results
                "user" if msg.tool_results.is_some() => {
                    if let Some(ref results) = msg.tool_results {
                        for result in results {
                            let id = result
                                .get("tool_use_id")
                                .and_then(|v| v.as_str())
                                .unwrap_or("?");
                            let content =
                                result.get("content").and_then(|v| v.as_str()).unwrap_or("");
                            let is_error = result
                                .get("is_error")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false);
                            let tag = if is_error { "ERROR" } else { "ok" };
                            log.push(format!("\n  ◀ TOOL RESULT [{id}] ({tag}):"));
                            log.push(format!("  {}", "─".repeat(60)));
                            let truncated = truncate_str(content, 3000);
                            for line in truncated.lines() {
                                log.push(format!("  │ {line}"));
                            }
                            log.push(format!("  {}", "─".repeat(60)));
                        }
                    }
                }
                role => {
                    let role_upper = role.to_uppercase();
                    log.push(format!("\n┌─ #{turn} [{role_upper}] {ts}"));

                    if !msg.content.trim().is_empty() {
                        log.push("│".to_string());
                        for line in msg.content.lines() {
                            log.push(format!("│ {line}"));
                        }
                    }

                    if let Some(ref calls) = msg.tool_calls {
                        if !msg.content.trim().is_empty() {
                            log.push("│".to_string());
                        }
                        for call in calls {
                            tool_call_count += 1;
                            let name = call.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                            let id = call.get("id").and_then(|v| v.as_str()).unwrap_or("?");
                            let input = call.get("input");
                            let input_str = input
                                .map(|v| {
                                    serde_json::to_string_pretty(v)
                                        .unwrap_or_else(|_| v.to_string())
                                })
                                .unwrap_or_default();
                            log.push(format!("│  ▶ {name} [{id}]"));
                            let truncated = truncate_str(&input_str, 2000);
                            for line in truncated.lines() {
                                log.push(format!("│    {line}"));
                            }
                        }
                    }
                    log.push("└─".to_string());
                }
            }
        }

        // Summary footer
        log.push(format!("\n{}", "─".repeat(72)));
        log.push(format!(
            "  SUMMARY: {turn} messages, {tool_call_count} tool calls"
        ));
        log.push("─".repeat(72));
    }

    /// Add a custom note to the diagnostic log.
    pub fn log(&self, msg: impl std::fmt::Display) {
        self.diagnostic_log.borrow_mut().push(format!("# {msg}"));
    }

    // -----------------------------------------------------------------
    // Session helpers
    // -----------------------------------------------------------------

    /// Create a bare session.
    pub async fn create_session(&self) -> Thing {
        ghost::db::sessions::create_session(&self.db)
            .await
            .expect("create session")
    }

    /// Create a session with pre-filled messages.
    pub async fn session_with_messages(&self, messages: &[(&str, &str)]) -> Thing {
        let session_id = self.create_session().await;
        for (role, content) in messages {
            ghost::db::sessions::create_message(&self.db, &session_id, role, content)
                .await
                .expect("create message");
        }
        session_id
    }

    // -----------------------------------------------------------------
    // Chat helpers
    // -----------------------------------------------------------------

    /// SessionChat with real provider + chat tools + agent runner.
    pub fn chat(&self) -> ghost::chat::SessionChat {
        ghost::chat::SessionChat::from_config(self.db.clone(), self.config.clone())
            .expect("build session chat")
            .with_agent_runner(Arc::clone(&self.agent_runner))
    }

    /// SessionChat with real provider + reflection tools.
    pub fn reflection_chat(&self) -> ghost::chat::SessionChat {
        ghost::chat::SessionChat::new(
            self.db.clone(),
            ghost::providers::provider_for_alias(&self.config, None).expect("provider"),
            ghost::tools::ToolManager::for_reflection(),
            self.config.clone(),
        )
    }

    // -----------------------------------------------------------------
    // Job runners
    // -----------------------------------------------------------------

    /// Run heartbeat on a session (loads prompt, calls chat_job).
    pub async fn run_heartbeat(&self, session_id: &Thing) -> ghost::chat::JobTranscript {
        let chat = self.chat();
        let prompt = ghost::jobs::heartbeat::load_prompt(
            &self.config.workspace,
            "heartbeat.md",
            "# Heartbeat Check\n\n\
             You are running a heartbeat check. The OPERATOR has been idle.\n\n\
             If there's nothing meaningful to say, respond with exactly: \
             HEARTBEAT_CONTINUE",
        );
        chat.chat_job(
            "heartbeat",
            &session_id.to_string(),
            &prompt,
            ghost::tools::ToolSet::Chat,
        )
        .await
        .expect("heartbeat chat_job")
    }

    /// Run reflection on a session (loads prompt, interpolates template,
    /// calls chat_job with reflection tools).
    pub async fn run_reflection(
        &self,
        session_id: &Thing,
        previous_handoff: Option<&str>,
    ) -> ghost::chat::JobTranscript {
        let messages = ghost::db::sessions::list_messages_by_session(&self.db, session_id)
            .await
            .expect("list messages");
        let transcript = ghost::jobs::reflection::filter_transcript(&messages);

        let renderer = ghost::prompt::PromptRenderer::new(self.config.clone());
        let prompt_body = ghost::jobs::heartbeat::load_prompt(
            &self.config.workspace,
            "reflection.md",
            ghost::jobs::reflection::DEFAULT_REFLECTION_PROMPT,
        );
        let web_cache_files =
            ghost::web::scan_web_cache(&self.config.workspace).expect("scan web cache");

        let interpolated = renderer
            .render_job_prompt(
                "reflection",
                &ghost::prompt::JobPromptContext {
                    prompt_body,
                    previous_handoff: previous_handoff.map(String::from),
                    diary_today: None,
                    recent_messages: Some(transcript),
                    web_cache_files,
                },
            )
            .expect("render reflection prompt");

        let reflection_chat = self.reflection_chat();
        let temp_session = self.create_session().await;

        reflection_chat
            .chat_job(
                "reflection",
                &temp_session.to_string(),
                &interpolated,
                ghost::tools::ToolSet::Reflection,
            )
            .await
            .expect("reflection chat_job")
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
        session_id: &Thing,
        timeout_secs: u64,
    ) -> Option<ghost::chat::ChatResult> {
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
                if let Some((status, parent)) = self.agent_runner.take_completed(agent_id).await {
                    let findings = status
                        .findings
                        .as_deref()
                        .unwrap_or("Agent completed without producing findings.");

                    self.log(format!(
                        "agent '{}' completed ({} messages)",
                        status.agent_name, status.message_count
                    ));

                    // Log the agent's session for diagnostics
                    // agent_id is "session:xxxxx" — parse it back into a Thing
                    if let Some((table, id)) = agent_id.split_once(':') {
                        let agent_session_thing = surrealdb::sql::Thing::from((table, id));
                        self.log_session("agent_session", &agent_session_thing)
                            .await;
                    }

                    let parent_id = parent.unwrap_or_else(|| session_id.clone());

                    // Inject findings as system message
                    let system_msg =
                        format!("[agent:{} completed]\n\n{findings}", status.agent_name);
                    ghost::db::sessions::create_message(
                        &self.db,
                        &parent_id,
                        "system",
                        &system_msg,
                    )
                    .await
                    .expect("inject agent findings");

                    // Trigger follow-up chat turn
                    let chat = self.chat();
                    let trigger = "[system] Research agent completed.";
                    match chat.chat(&parent_id.to_string(), trigger).await {
                        Ok(result) => return Some(result),
                        Err(e) => {
                            self.log(format!("ERROR: follow-up chat turn failed: {e}"));
                            return None;
                        }
                    }
                }
            }

            tokio::time::sleep(poll_interval).await;
        }
    }

    // -----------------------------------------------------------------
    // Assertion helpers
    // -----------------------------------------------------------------

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

    /// Recursively search for any file under a workspace subdirectory
    /// whose content contains a string.
    pub fn find_file_containing(&self, dir: &str, needle: &str) -> bool {
        find_file_containing_recursive(&self.workspace.path().join(dir), needle)
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

        // Write diagnostic log
        let log = self.diagnostic_log.borrow();
        if !log.is_empty() {
            let log_content = log.join("\n");
            let log_path = dest.join("diagnostic.log");
            if let Err(e) = fs::write(&log_path, &log_content) {
                eprintln!("warning: failed to write diagnostic log: {e}");
            }
            // Also print to stderr so --nocapture shows it
            eprintln!("\n--- diagnostic log ({}) ---", self.test_name);
            eprintln!("{log_content}");
            eprintln!("--- end diagnostic log ---\n");
        }

        eprintln!("e2e snapshot: {}", dest.display());

        // Restore env vars
        match &self.prev_config_dir_env {
            Some(val) => unsafe { std::env::set_var(ghost::config::CONFIG_DIR_ENV, val) },
            None => unsafe { std::env::remove_var(ghost::config::CONFIG_DIR_ENV) },
        }
        if let Some(val) = &self.prev_path_env {
            unsafe { std::env::set_var("PATH", val) };
        }
    }
}

/// Create a live test environment: real provider from `~/.config/ghost/`,
/// fresh temp workspace + database, `GHOST_CONFIG_DIR` set so spawned
/// `ghost` subprocesses use the temp workspace.
#[cfg(feature = "live-tests")]
#[allow(dead_code)]
pub async fn live_test_database(test_name: &str) -> LiveTestEnv {
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
    unsafe {
        std::env::set_var(ghost::config::CONFIG_DIR_ENV, config_dir.path());

        // Add target/debug to PATH so `ghost web fetch` works in subprocess
        let target_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/debug");
        let path = std::env::var("PATH").unwrap_or_default();
        std::env::set_var("PATH", format!("{}:{}", target_dir.display(), path));
    }

    // Load config from temp dir + bootstrap + connect
    let config = config::load_from_dir(config_dir.path()).expect("load config from temp dir");
    ghost::config_workspace::bootstrap_workspace(&config).expect("bootstrap temp workspace");
    let db = db::connect(&config.workspace)
        .await
        .expect("connect to fresh temp database");

    let agent_runner = Arc::new(ghost::agents::AgentRunner::new(db.clone(), config.clone()));

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
    }
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
        format!("{}…[{}b total]", &s[..max], s.len())
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
#[allow(dead_code)]
pub struct MockProvider {
    responses: Arc<Mutex<VecDeque<ChatResponse>>>,
    requests: Arc<Mutex<Vec<ChatRequest>>>,
}

#[allow(dead_code)]
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
#[allow(dead_code)]
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
    ) -> Result<String, ToolError> {
        let text = params
            .get("text")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        Ok(format!("echo:{text}"))
    }
}

#[allow(dead_code)]
pub fn response(content: Vec<ContentBlock>, stop_reason: StopReason) -> ChatResponse {
    ChatResponse {
        content,
        usage: Usage::default(),
        stop_reason,
        model: "mock-model".to_string(),
    }
}

/// Build a mock response that calls the `respond` output tool.
#[allow(dead_code)]
pub fn respond_response(message: &str, citations: Vec<serde_json::Value>) -> ChatResponse {
    response(
        vec![ContentBlock::ToolUse {
            id: "respond_1".to_string(),
            name: "respond".to_string(),
            input: json!({"message": message, "citations": citations}),
        }],
        StopReason::ToolUse,
    )
}
