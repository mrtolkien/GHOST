use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use regex::Regex;
use std::sync::LazyLock;
use surrealdb::Surreal;
use surrealdb::engine::local::Db;
use surrealdb::types::RecordId;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::chat::{RunMetadata, SessionChat};
use crate::config::Config;
use crate::db;
use crate::providers::provider_for_alias;
use crate::tools::ToolManager;

use super::definition::{TaskDefinition, load_task};
use super::error::TaskError;

/// Status snapshot of a running or completed agent.
#[derive(Debug, Clone)]
pub struct TaskStatus {
    pub agent_id: String,
    pub agent_name: String,
    pub status: String,
    pub message_count: usize,
    pub todo_summary: Option<String>,
    pub findings: Option<String>,
    pub metadata: Option<RunMetadata>,
}

/// Handle for a spawned agent task.
struct TaskHandle {
    agent_id: String,
    agent_name: String,
    parent_session_id: Option<RecordId>,
    agent_session_id: RecordId,
    job_log_id: RecordId,
    task_handle: JoinHandle<()>,
    cancel_token: CancellationToken,
    metadata: Arc<Mutex<Option<RunMetadata>>>,
}

/// Manages background agent execution.
#[derive(Debug, Clone)]
pub struct TaskRunner {
    db: Surreal<Db>,
    config: Config,
    handles: Arc<Mutex<HashMap<String, TaskHandle>>>,
}

impl TaskRunner {
    #[must_use]
    pub fn new(db: Surreal<Db>, config: Config) -> Self {
        Self {
            db,
            config,
            handles: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Start a background agent.
    ///
    /// Returns the agent_id on success.
    #[tracing::instrument(name = "start agent", skip_all, fields(
        gen_ai.agent.name = %agent_name,
    ))]
    pub async fn start(
        &self,
        agent_name: &str,
        prompt: &str,
        parent_session_id: Option<&RecordId>,
    ) -> Result<String, TaskError> {
        let definition = load_task(&self.config.workspace, agent_name)?;

        // Create agent session
        let agent_session_id = db::sessions::create_agent_session(&self.db).await?;
        let agent_id = crate::db::fmt_id(&agent_session_id);

        // Create job_log
        let job_log_id = db::job_logs::create_agent_job_log(
            &self.db,
            agent_name,
            parent_session_id,
            &agent_session_id,
        )
        .await?;

        let cancel_token = CancellationToken::new();
        let metadata_slot: Arc<Mutex<Option<RunMetadata>>> = Arc::new(Mutex::new(None));

        let handle = TaskHandle {
            agent_id: agent_id.clone(),
            agent_name: agent_name.to_string(),
            parent_session_id: parent_session_id.cloned(),
            agent_session_id: agent_session_id.clone(),
            job_log_id: job_log_id.clone(),
            task_handle: self.spawn_task(
                definition,
                prompt.to_string(),
                agent_session_id,
                job_log_id,
                cancel_token.clone(),
                Arc::clone(&metadata_slot),
            ),
            cancel_token,
            metadata: metadata_slot,
        };

        self.handles.lock().await.insert(agent_id.clone(), handle);

        logfire::info!(
            "agent started",
            agent_name = agent_name.to_string(),
            agent_id = agent_id.clone(),
        );

        Ok(agent_id)
    }

    /// Run an agent synchronously (await completion).
    ///
    /// Creates DB records (session, job_log), runs the agent, finishes
    /// job_log. Returns the final findings message.
    #[tracing::instrument(name = "run agent", skip_all, fields(
        gen_ai.agent.name = %agent_name,
        gen_ai.operation.name = "invoke_agent",
    ))]
    pub async fn run_to_completion(
        &self,
        agent_name: &str,
        prompt: &str,
        parent_session_id: Option<&RecordId>,
    ) -> Result<String, TaskError> {
        let definition = load_task(&self.config.workspace, agent_name)?;

        let agent_session_id = db::sessions::create_agent_session(&self.db).await?;

        let job_log_id = db::job_logs::create_agent_job_log(
            &self.db,
            agent_name,
            parent_session_id,
            &agent_session_id,
        )
        .await?;

        let cancel_token = CancellationToken::new();

        let result = run_task(
            &self.db,
            &self.config,
            &definition,
            prompt,
            &agent_session_id,
            &cancel_token,
        )
        .await;

        let (status, transcript) = match result {
            Ok((findings, _meta)) => ("ok", findings),
            Err(e) => {
                logfire::error!(
                    "agent run_to_completion failed",
                    agent_name = agent_name.to_string(),
                    error = e.to_string(),
                );
                ("failed", format!("Agent error: {e}"))
            }
        };

        if let Err(e) =
            db::job_logs::finish_job_log(&self.db, &job_log_id, status, &transcript).await
        {
            logfire::error!("failed to finish agent job_log", error = e.to_string(),);
        }

        if status == "failed" {
            return Err(TaskError::ExecutionFailed {
                message: transcript.to_string(),
            });
        }

        Ok(transcript)
    }

    /// Run a pre-parsed agent definition synchronously (await completion).
    ///
    /// Like `run_to_completion`, but accepts a definition directly instead
    /// of loading it by name. Useful for cron jobs and other callers that
    /// already have a parsed definition.
    #[tracing::instrument(name = "run agent", skip_all, fields(
        gen_ai.agent.name = %definition.name,
        gen_ai.operation.name = "invoke_agent",
    ))]
    pub async fn run_definition_to_completion(
        &self,
        definition: &TaskDefinition,
        prompt: &str,
        parent_session_id: Option<&RecordId>,
    ) -> Result<String, TaskError> {
        let agent_session_id = db::sessions::create_agent_session(&self.db).await?;

        let job_log_id = db::job_logs::create_agent_job_log(
            &self.db,
            &definition.name,
            parent_session_id,
            &agent_session_id,
        )
        .await?;

        let cancel_token = CancellationToken::new();

        let result = run_task(
            &self.db,
            &self.config,
            definition,
            prompt,
            &agent_session_id,
            &cancel_token,
        )
        .await;

        let (status, transcript) = match result {
            Ok((findings, _meta)) => ("ok", findings),
            Err(e) => {
                logfire::error!(
                    "agent run_definition_to_completion failed",
                    agent_name = definition.name.clone(),
                    error = e.to_string(),
                );
                ("failed", format!("Agent error: {e}"))
            }
        };

        if let Err(e) =
            db::job_logs::finish_job_log(&self.db, &job_log_id, status, &transcript).await
        {
            logfire::error!("failed to finish agent job_log", error = e.to_string(),);
        }

        if status == "failed" {
            return Err(TaskError::ExecutionFailed {
                message: transcript.to_string(),
            });
        }

        Ok(transcript)
    }

    /// Check agent status.
    pub async fn status(&self, agent_id: &str) -> Result<TaskStatus, TaskError> {
        let handles = self.handles.lock().await;
        let handle = handles
            .get(agent_id)
            .ok_or_else(|| TaskError::AgentNotFound {
                agent_id: agent_id.to_string(),
            })?;

        let is_finished = handle.task_handle.is_finished();

        // Read message count from agent session
        let message_count =
            db::sessions::count_messages_for_session(&self.db, &handle.agent_session_id)
                .await
                .unwrap_or(0);

        // Read TODO list from agent session
        let todo_summary = db::sessions::get_session_todo_list(&self.db, &handle.agent_session_id)
            .await
            .ok()
            .flatten()
            .map(|items| crate::tools::format_todo_list(&items));

        // Read findings from job_log transcript if finished
        let findings = if is_finished {
            self.get_job_transcript(&handle.job_log_id).await
        } else {
            None
        };

        let status = if is_finished { "completed" } else { "running" };

        let metadata = if is_finished {
            handle.metadata.lock().await.clone()
        } else {
            None
        };

        Ok(TaskStatus {
            agent_id: agent_id.to_string(),
            agent_name: handle.agent_name.clone(),
            status: status.to_string(),
            message_count,
            todo_summary,
            findings,
            metadata,
        })
    }

    /// Stop a running agent and return partial findings.
    pub async fn stop(&self, agent_id: &str) -> Result<TaskStatus, TaskError> {
        let handles = self.handles.lock().await;
        let handle = handles
            .get(agent_id)
            .ok_or_else(|| TaskError::AgentNotFound {
                agent_id: agent_id.to_string(),
            })?;

        // Signal cancellation
        handle.cancel_token.cancel();
        let agent_name = handle.agent_name.clone();
        let agent_session_id = handle.agent_session_id.clone();
        let job_log_id = handle.job_log_id.clone();
        drop(handles);

        // Give the task a moment to finish
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let message_count = db::sessions::count_messages_for_session(&self.db, &agent_session_id)
            .await
            .unwrap_or(0);

        let todo_summary = db::sessions::get_session_todo_list(&self.db, &agent_session_id)
            .await
            .ok()
            .flatten()
            .map(|items| crate::tools::format_todo_list(&items));

        let findings = self.get_job_transcript(&job_log_id).await;

        // Clean up handle
        let agent_id_owned = agent_id.to_string();
        self.handles.lock().await.remove(agent_id);

        logfire::info!(
            "agent stopped",
            agent_name = agent_name.clone(),
            agent_id = agent_id_owned.clone(),
        );

        Ok(TaskStatus {
            agent_id: agent_id.to_string(),
            agent_name,
            status: "stopped".to_string(),
            message_count,
            todo_summary,
            findings,
            metadata: None,
        })
    }

    /// Get findings for a completed agent, cleaning up its handle.
    pub async fn take_completed(&self, agent_id: &str) -> Option<(TaskStatus, Option<RecordId>)> {
        let mut handles = self.handles.lock().await;
        let handle = handles.get(agent_id)?;
        if !handle.task_handle.is_finished() {
            return None;
        }
        let parent = handle.parent_session_id.clone();
        let agent_name = handle.agent_name.clone();
        let agent_session_id = handle.agent_session_id.clone();
        let job_log_id = handle.job_log_id.clone();
        let metadata = handle.metadata.lock().await.clone();
        handles.remove(agent_id);
        drop(handles);

        let message_count = db::sessions::count_messages_for_session(&self.db, &agent_session_id)
            .await
            .unwrap_or(0);

        let todo_summary = db::sessions::get_session_todo_list(&self.db, &agent_session_id)
            .await
            .ok()
            .flatten()
            .map(|items| crate::tools::format_todo_list(&items));

        let findings = self.get_job_transcript(&job_log_id).await;

        Some((
            TaskStatus {
                agent_id: agent_id.to_string(),
                agent_name,
                status: "completed".to_string(),
                message_count,
                todo_summary,
                findings,
                metadata,
            },
            parent,
        ))
    }

    /// Continue an existing agent session with a new prompt.
    ///
    /// Looks up the agent name from the job_log, loads the agent definition,
    /// creates a new job_log entry for this continuation, and spawns a task
    /// that resumes the existing session with full history.
    #[tracing::instrument(name = "continue agent", skip_all, fields(agent_id = agent_id))]
    pub async fn continue_task(
        &self,
        agent_id: &str,
        prompt: &str,
        parent_session_id: Option<&RecordId>,
    ) -> Result<String, TaskError> {
        // Parse agent_id as a session RecordId
        let agent_session_id = parse_task_session_thing(agent_id)?;

        // Look up agent name from job_log
        let agent_name = db::job_logs::get_agent_name_for_session(&self.db, &agent_session_id)
            .await?
            .ok_or_else(|| TaskError::AgentSessionNotFound {
                agent_session_id: agent_id.to_string(),
            })?;

        // Load agent definition from workspace
        let definition = load_task(&self.config.workspace, &agent_name)?;

        // Create new job_log entry for this continuation
        let job_log_id = db::job_logs::create_agent_job_log(
            &self.db,
            &agent_name,
            parent_session_id,
            &agent_session_id,
        )
        .await?;

        let cancel_token = CancellationToken::new();
        let metadata_slot: Arc<Mutex<Option<RunMetadata>>> = Arc::new(Mutex::new(None));

        let handle = TaskHandle {
            agent_id: agent_id.to_string(),
            agent_name: agent_name.clone(),
            parent_session_id: parent_session_id.cloned(),
            agent_session_id: agent_session_id.clone(),
            job_log_id: job_log_id.clone(),
            task_handle: self.spawn_continue_task(
                definition,
                prompt.to_string(),
                agent_session_id,
                job_log_id,
                cancel_token.clone(),
                Arc::clone(&metadata_slot),
            ),
            cancel_token,
            metadata: metadata_slot,
        };

        self.handles
            .lock()
            .await
            .insert(agent_id.to_string(), handle);

        logfire::info!(
            "agent continued",
            agent_name = agent_name.clone(),
            agent_id = agent_id.to_string(),
        );

        Ok(agent_name)
    }

    /// List all agent IDs (running and completed).
    pub async fn list_task_ids(&self) -> Vec<String> {
        self.handles.lock().await.keys().cloned().collect()
    }

    fn spawn_task(
        &self,
        definition: TaskDefinition,
        prompt: String,
        agent_session_id: RecordId,
        job_log_id: RecordId,
        cancel_token: CancellationToken,
        metadata_slot: Arc<Mutex<Option<RunMetadata>>>,
    ) -> JoinHandle<()> {
        let db = self.db.clone();
        let config = self.config.clone();

        tokio::spawn(async move {
            let result = run_task(
                &db,
                &config,
                &definition,
                &prompt,
                &agent_session_id,
                &cancel_token,
            )
            .await;

            let (status, transcript) = match result {
                Ok((findings, meta)) => {
                    *metadata_slot.lock().await = Some(meta);
                    ("ok", findings)
                }
                Err(e) => {
                    logfire::error!(
                        "agent failed",
                        agent_name = definition.name.clone(),
                        error = e.to_string(),
                    );
                    ("failed", format!("Agent error: {e}"))
                }
            };

            if let Err(e) =
                db::job_logs::finish_job_log(&db, &job_log_id, status, &transcript).await
            {
                logfire::error!("failed to finish agent job_log", error = e.to_string(),);
            }
        })
    }

    fn spawn_continue_task(
        &self,
        definition: TaskDefinition,
        prompt: String,
        agent_session_id: RecordId,
        job_log_id: RecordId,
        cancel_token: CancellationToken,
        metadata_slot: Arc<Mutex<Option<RunMetadata>>>,
    ) -> JoinHandle<()> {
        let db = self.db.clone();
        let config = self.config.clone();

        tokio::spawn(async move {
            let result = continue_task_run(
                &db,
                &config,
                &definition,
                &prompt,
                &agent_session_id,
                &cancel_token,
            )
            .await;

            let (status, transcript) = match result {
                Ok((findings, meta)) => {
                    *metadata_slot.lock().await = Some(meta);
                    ("ok", findings)
                }
                Err(e) => {
                    logfire::error!(
                        "agent continuation failed",
                        agent_name = definition.name.clone(),
                        error = e.to_string(),
                    );
                    ("failed", format!("Agent error: {e}"))
                }
            };

            if let Err(e) =
                db::job_logs::finish_job_log(&db, &job_log_id, status, &transcript).await
            {
                logfire::error!("failed to finish agent job_log", error = e.to_string(),);
            }
        })
    }

    async fn get_job_transcript(&self, job_log_id: &RecordId) -> Option<String> {
        let logs = db::job_logs::list_job_logs(&self.db, None, 100)
            .await
            .ok()?;
        logs.into_iter()
            .find(|log| log.id == *job_log_id)
            .and_then(|log| log.transcript)
    }
}

static SKILL_PLACEHOLDER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\{\{\s*skill:([a-z0-9_-]+)\s*\}\}").expect("skill regex"));

/// Resolve `{{ skill:name }}` placeholders in a system prompt by inlining
/// the skill file's body (YAML frontmatter stripped).
///
/// Reads `$WORKSPACE/skills/{name}/skill.md`, strips the `---` frontmatter
/// block, and substitutes the body in place. Logs a warning if a referenced
/// skill file is not found.
fn interpolate_skill_content(system_prompt: &str, workspace: &Path) -> String {
    SKILL_PLACEHOLDER_RE
        .replace_all(system_prompt, |caps: &regex::Captures| {
            let skill_name = &caps[1];
            let skill_path = workspace.join("skills").join(skill_name).join("skill.md");

            match std::fs::read_to_string(&skill_path) {
                Ok(content) => strip_yaml_frontmatter(&content),
                Err(_) => {
                    logfire::warn!(
                        "skill interpolation: file not found",
                        skill = skill_name.to_string(),
                        path = skill_path.display().to_string(),
                    );
                    format!("[skill '{skill_name}' not found]")
                }
            }
        })
        .into_owned()
}

/// Strip YAML frontmatter (--- ... ---) from skill content, returning the
/// body after the closing delimiter.
fn strip_yaml_frontmatter(content: &str) -> String {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return content.to_string();
    }

    let after_open = &trimmed[3..];
    if let Some(close) = after_open.find("\n---") {
        let body_start = close + 4; // skip "\n---"
        after_open[body_start..]
            .trim_start_matches('\n')
            .to_string()
    } else {
        content.to_string()
    }
}

/// Build a skills section to append to the agent system prompt.
///
/// If the agent definition declares `skills = [...]`, discover skills from
/// the workspace, filter to matching names, and build an XML block (same
/// format as `build_ghost_skills` in `prompt::context`).
fn build_agent_skills_section(config: &Config, skills: &[String]) -> String {
    if skills.is_empty() {
        return String::new();
    }

    let discovered = crate::skills::discover_skills(&config.workspace);
    let matched: Vec<_> = discovered
        .iter()
        .filter(|s| skills.contains(&s.name))
        .collect();

    if matched.is_empty() {
        return String::new();
    }

    let entries: Vec<String> = matched
        .iter()
        .map(|s| {
            let rel = s
                .path
                .strip_prefix(&config.workspace)
                .unwrap_or(&s.path)
                .display();
            format!(
                "  <skill>\n    <name>{}</name>\n    \
                 <description>{}</description>\n    \
                 <location>{rel}</location>\n  </skill>",
                s.name, s.description,
            )
        })
        .collect();

    format!(
        "\n\n## Available Skills\n\n\
         ALWAYS read the full skill file with `read_file` before starting any task \
         that matches a skill's description. Skills contain critical workflow \
         instructions that you MUST follow.\n\n\
         <available_skills>\n{}\n</available_skills>",
        entries.join("\n"),
    )
}

/// Execute the agent tool loop. Returns the final findings string.
#[tracing::instrument(name = "run agent", skip_all, fields(
    gen_ai.agent.name = %definition.name,
    gen_ai.agent.id = ?agent_session_id,
    gen_ai.operation.name = "invoke_agent",
))]
async fn run_task(
    db: &Surreal<Db>,
    config: &Config,
    definition: &TaskDefinition,
    prompt: &str,
    agent_session_id: &RecordId,
    cancel_token: &CancellationToken,
) -> Result<(String, RunMetadata), TaskError> {
    let mut system_prompt = definition.render_system_prompt(prompt);
    system_prompt = interpolate_skill_content(&system_prompt, &config.workspace);

    // Append skills section if the agent declares any
    system_prompt.push_str(&build_agent_skills_section(config, &definition.skills));

    // Resolve provider — use agent's model alias or default
    let provider = provider_for_alias(config, definition.model.as_deref())?;

    // Build restricted tool manager — include read_file if skills are declared
    let mut tools = definition.tools.clone();
    if !definition.skills.is_empty() && !tools.iter().any(|t| t == "read_file") {
        tools.push("read_file".to_string());
    }
    let tool_manager = ToolManager::for_agent(&tools);

    let session_chat = SessionChat::new(db.clone(), provider, tool_manager, config.clone())
        .with_max_tool_iterations(definition.max_iterations);

    let session_id = crate::db::fmt_id(agent_session_id);

    // Run with cancellation support
    let result = tokio::select! {
        res = session_chat.chat_agent(
            &session_id,
            prompt,
            system_prompt,
            definition,
            None,
        ) => res?,
        () = cancel_token.cancelled() => {
            logfire::info!("agent cancelled", agent_name = definition.name.clone());
            // Return partial findings from session
            let messages = db::sessions::list_messages_by_session(db, agent_session_id)
                .await
                .unwrap_or_default();
            let last_assistant = messages
                .iter()
                .rev()
                .find(|m| m.role == "assistant" && !m.content.is_empty())
                .map(|m| m.content.clone())
                .unwrap_or_else(|| "Agent was cancelled before producing findings.".to_string());
            return Ok((last_assistant, RunMetadata::default()));
        }
    };

    Ok((result.0.message, result.1))
}

/// Continue an existing agent session with a new prompt. Loads full history
/// from DB instead of starting fresh.
#[tracing::instrument(name = "continue agent", skip_all, fields(
    gen_ai.agent.name = %definition.name,
    gen_ai.agent.id = ?agent_session_id,
    gen_ai.operation.name = "invoke_agent",
))]
async fn continue_task_run(
    db: &Surreal<Db>,
    config: &Config,
    definition: &TaskDefinition,
    prompt: &str,
    agent_session_id: &RecordId,
    cancel_token: &CancellationToken,
) -> Result<(String, RunMetadata), TaskError> {
    // For continuation, interpolate a generic marker instead of the new prompt
    // into the system prompt template, since the original query is already in
    // the session history.
    let mut system_prompt = definition.render_system_prompt(prompt);
    system_prompt = interpolate_skill_content(&system_prompt, &config.workspace);
    system_prompt.push_str(&build_agent_skills_section(config, &definition.skills));

    let provider = provider_for_alias(config, definition.model.as_deref())?;
    let mut tools = definition.tools.clone();
    if !definition.skills.is_empty() && !tools.iter().any(|t| t == "read_file") {
        tools.push("read_file".to_string());
    }
    let tool_manager = ToolManager::for_agent(&tools);

    let session_chat = SessionChat::new(db.clone(), provider, tool_manager, config.clone())
        .with_max_tool_iterations(definition.max_iterations);

    let session_id = crate::db::fmt_id(agent_session_id);

    let result = tokio::select! {
        res = session_chat.continue_task(
            &session_id,
            prompt,
            system_prompt,
            definition,
            None,
        ) => res?,
        () = cancel_token.cancelled() => {
            logfire::info!(
                "agent continuation cancelled",
                agent_name = definition.name.clone(),
            );
            let messages = db::sessions::list_messages_by_session(db, agent_session_id)
                .await
                .unwrap_or_default();
            let last_assistant = messages
                .iter()
                .rev()
                .find(|m| m.role == "assistant" && !m.content.is_empty())
                .map(|m| m.content.clone())
                .unwrap_or_else(|| {
                    "Agent was cancelled before producing findings.".to_string()
                });
            return Ok((last_assistant, RunMetadata::default()));
        }
    };

    Ok((result.0.message, result.1))
}

/// Parse an agent_id string (e.g. "session:abc123") into a RecordId.
fn parse_task_session_thing(agent_id: &str) -> Result<RecordId, TaskError> {
    let (table, id) = agent_id
        .split_once(':')
        .ok_or_else(|| TaskError::AgentSessionNotFound {
            agent_session_id: agent_id.to_string(),
        })?;
    if table.is_empty() || id.is_empty() {
        return Err(TaskError::AgentSessionNotFound {
            agent_session_id: agent_id.to_string(),
        });
    }
    Ok(RecordId::new(table, id))
}

impl std::fmt::Debug for TaskHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TaskHandle")
            .field("agent_id", &self.agent_id)
            .field("agent_name", &self.agent_name)
            .field("finished", &self.task_handle.is_finished())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpolate_skill_replaces_placeholder() {
        let dir = tempfile::TempDir::new().unwrap();
        let skill_dir = dir.path().join("skills").join("note-writer");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("skill.md"),
            "---\nname: note-writer\ndescription: Write notes.\n---\n\n# Note Guide\n\nWrite good notes.",
        )
        .unwrap();

        let input = "Before\n\n{{ skill:note-writer }}\n\nAfter";
        let result = interpolate_skill_content(input, dir.path());
        assert!(result.contains("# Note Guide"));
        assert!(result.contains("Write good notes."));
        assert!(result.contains("Before"));
        assert!(result.contains("After"));
        // Frontmatter should be stripped
        assert!(!result.contains("name: note-writer"));
    }

    #[test]
    fn interpolate_skill_missing_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let input = "Use {{ skill:nonexistent }} here";
        let result = interpolate_skill_content(input, dir.path());
        assert!(result.contains("[skill 'nonexistent' not found]"));
    }

    #[test]
    fn interpolate_skill_no_placeholders() {
        let dir = tempfile::TempDir::new().unwrap();
        let input = "No placeholders here";
        let result = interpolate_skill_content(input, dir.path());
        assert_eq!(result, input);
    }

    #[test]
    fn strip_yaml_frontmatter_removes_block() {
        let content = "---\nname: test\ndescription: Test.\n---\n\n# Body\n\nContent.";
        let result = strip_yaml_frontmatter(content);
        assert_eq!(result, "# Body\n\nContent.");
    }

    #[test]
    fn strip_yaml_frontmatter_no_frontmatter() {
        let content = "# Just a body\n\nNo frontmatter.";
        let result = strip_yaml_frontmatter(content);
        assert_eq!(result, content);
    }
}
