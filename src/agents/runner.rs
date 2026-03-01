use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::chat::{RunMetadata, SessionChat};
use crate::config::Config;
use crate::db;
use crate::db::GhostDb;
use crate::providers::provider_for_alias;
use crate::scripting::AgentContext;
use crate::scripting::build_custom_tools;
use crate::tools::ToolManager;

use super::error::AgentError;
use super::loader::load_agent_with_host;

/// Status snapshot of a running or completed agent.
#[derive(Debug, Clone)]
pub struct AgentStatus {
    pub agent_id: String,
    pub agent_name: String,
    pub status: String,
    pub message_count: usize,
    pub todo_summary: Option<String>,
    pub findings: Option<String>,
    pub metadata: Option<RunMetadata>,
}

/// Handle for a spawned agent task.
struct AgentHandle {
    agent_id: String,
    agent_name: String,
    parent_session_id: Option<String>,
    agent_session_id: String,
    run_id: String,
    join_handle: JoinHandle<()>,
    cancel_token: CancellationToken,
    metadata: Arc<Mutex<Option<RunMetadata>>>,
}

/// Manages background agent execution.
#[derive(Debug, Clone)]
pub struct AgentRunner {
    db: GhostDb,
    config: Config,
    handles: Arc<Mutex<HashMap<String, AgentHandle>>>,
}

impl AgentRunner {
    #[must_use]
    pub fn new(db: GhostDb, config: Config) -> Self {
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
        parent_session_id: Option<&str>,
    ) -> Result<String, AgentError> {
        // Validate the agent exists
        let agent_dir = self.config.workspace.join("agents").join(agent_name);
        if !agent_dir.join("agent.lua").exists() {
            return Err(AgentError::NotFound {
                name: agent_name.to_string(),
            });
        }

        // Create agent session
        let agent_session_id = db::sessions::create_agent_session(&self.db).await?;
        let agent_id = agent_session_id.clone();

        // Create agent run record
        let run_id = db::agent_runs::create_agent_run(
            &self.db,
            agent_name,
            parent_session_id,
            &agent_session_id,
        )
        .await?;

        let cancel_token = CancellationToken::new();
        let metadata_slot: Arc<Mutex<Option<RunMetadata>>> = Arc::new(Mutex::new(None));

        let handle = AgentHandle {
            agent_id: agent_id.clone(),
            agent_name: agent_name.to_string(),
            parent_session_id: parent_session_id.map(|s| s.to_string()),
            agent_session_id: agent_session_id.clone(),
            run_id: run_id.clone(),
            join_handle: self.spawn_run(
                agent_name.to_string(),
                prompt.to_string(),
                agent_session_id,
                run_id,
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
    /// Creates DB records (session, agent run), runs the agent, finishes
    /// the run. Returns the final findings message.
    pub async fn run_to_completion(
        &self,
        agent_name: &str,
        prompt: &str,
        parent_session_id: Option<&str>,
    ) -> Result<(String, RunMetadata), AgentError> {
        let agent_session_id = db::sessions::create_agent_session(&self.db).await?;

        let run_id = db::agent_runs::create_agent_run(
            &self.db,
            agent_name,
            parent_session_id,
            &agent_session_id,
        )
        .await?;

        let cancel_token = CancellationToken::new();

        let result = run_agent(
            &self.db,
            &self.config,
            agent_name,
            prompt,
            &agent_session_id,
            &cancel_token,
        )
        .await;

        let (status, transcript, metadata) = match result {
            Ok((findings, meta)) => ("ok", findings, meta),
            Err(e) => {
                logfire::error!(
                    "agent run_to_completion failed",
                    agent_name = agent_name.to_string(),
                    error = e.to_string(),
                );
                (
                    "failed",
                    format!("Agent error: {e}"),
                    RunMetadata::default(),
                )
            }
        };

        if let Err(e) = db::agent_runs::finish_run(&self.db, &run_id, status, &transcript).await {
            logfire::error!("failed to finish agent run", error = e.to_string(),);
        }

        if status == "failed" {
            return Err(AgentError::ExecutionFailed {
                message: transcript.to_string(),
            });
        }

        Ok((transcript, metadata))
    }

    /// Check agent status.
    pub async fn status(&self, agent_id: &str) -> Result<AgentStatus, AgentError> {
        let handles = self.handles.lock().await;
        let handle = handles
            .get(agent_id)
            .ok_or_else(|| AgentError::AgentNotFound {
                agent_id: agent_id.to_string(),
            })?;

        let is_finished = handle.join_handle.is_finished();

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

        // Read findings from agent run transcript if finished
        let findings = if is_finished {
            self.get_run_transcript(&handle.run_id).await
        } else {
            None
        };

        let status = if is_finished { "completed" } else { "running" };

        let metadata = if is_finished {
            handle.metadata.lock().await.clone()
        } else {
            None
        };

        Ok(AgentStatus {
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
    pub async fn stop(&self, agent_id: &str) -> Result<AgentStatus, AgentError> {
        let handles = self.handles.lock().await;
        let handle = handles
            .get(agent_id)
            .ok_or_else(|| AgentError::AgentNotFound {
                agent_id: agent_id.to_string(),
            })?;

        // Signal cancellation
        handle.cancel_token.cancel();
        let agent_name = handle.agent_name.clone();
        let agent_session_id = handle.agent_session_id.clone();
        let run_id = handle.run_id.clone();
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

        let findings = self.get_run_transcript(&run_id).await;

        // Clean up handle
        let agent_id_owned = agent_id.to_string();
        self.handles.lock().await.remove(agent_id);

        logfire::info!(
            "agent stopped",
            agent_name = agent_name.clone(),
            agent_id = agent_id_owned.clone(),
        );

        Ok(AgentStatus {
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
    pub async fn take_completed(&self, agent_id: &str) -> Option<(AgentStatus, Option<String>)> {
        let mut handles = self.handles.lock().await;
        let handle = handles.get(agent_id)?;
        if !handle.join_handle.is_finished() {
            return None;
        }
        let parent = handle.parent_session_id.clone();
        let agent_name = handle.agent_name.clone();
        let agent_session_id = handle.agent_session_id.clone();
        let run_id = handle.run_id.clone();
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

        let findings = self.get_run_transcript(&run_id).await;

        Some((
            AgentStatus {
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
    /// Looks up the agent name from previous runs, creates a new run entry
    /// for this continuation, and spawns a background run that resumes the existing
    /// session with full history.
    #[tracing::instrument(name = "continue agent", skip_all, fields(agent_id = agent_id))]
    pub async fn continue_agent(
        &self,
        agent_id: &str,
        prompt: &str,
        parent_session_id: Option<&str>,
    ) -> Result<String, AgentError> {
        // Parse agent_id to extract bare session ID
        let agent_session_id = parse_agent_session_id(agent_id)?;

        // Look up agent name from previous runs
        let agent_name = db::agent_runs::get_agent_name_for_session(&self.db, &agent_session_id)
            .await?
            .ok_or_else(|| AgentError::AgentSessionNotFound {
                agent_session_id: agent_id.to_string(),
            })?;

        // Create new run entry for this continuation
        let run_id = db::agent_runs::create_agent_run(
            &self.db,
            &agent_name,
            parent_session_id,
            &agent_session_id,
        )
        .await?;

        let cancel_token = CancellationToken::new();
        let metadata_slot: Arc<Mutex<Option<RunMetadata>>> = Arc::new(Mutex::new(None));

        let handle = AgentHandle {
            agent_id: agent_id.to_string(),
            agent_name: agent_name.clone(),
            parent_session_id: parent_session_id.map(|s| s.to_string()),
            agent_session_id: agent_session_id.clone(),
            run_id: run_id.clone(),
            join_handle: self.spawn_continue(
                agent_name.clone(),
                prompt.to_string(),
                agent_session_id,
                run_id,
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

    /// Continue an existing agent session synchronously (await completion).
    ///
    /// Like `continue_agent` but blocks until the agent finishes, returning
    /// the final findings string and run metadata. Used for reflection forks
    /// where we need the result before proceeding.
    #[tracing::instrument(name = "continue agent sync", skip_all, fields(
        agent_session_id = agent_session_id,
    ))]
    pub async fn continue_to_completion(
        &self,
        agent_session_id: &str,
        prompt: &str,
    ) -> Result<(String, RunMetadata), AgentError> {
        let agent_name = db::agent_runs::get_agent_name_for_session(&self.db, agent_session_id)
            .await?
            .ok_or_else(|| AgentError::AgentSessionNotFound {
                agent_session_id: agent_session_id.to_string(),
            })?;

        let run_id =
            db::agent_runs::create_agent_run(&self.db, &agent_name, None, agent_session_id).await?;

        let cancel_token = CancellationToken::new();

        let result = continue_agent_inner(
            &self.db,
            &self.config,
            &agent_name,
            prompt,
            agent_session_id,
            &cancel_token,
        )
        .await;

        let (status, transcript, metadata) = match result {
            Ok((findings, meta)) => ("ok", findings, meta),
            Err(e) => {
                logfire::error!(
                    "agent continue_to_completion failed",
                    agent_name = agent_name.clone(),
                    error = e.to_string(),
                );
                (
                    "failed",
                    format!("Agent error: {e}"),
                    RunMetadata::default(),
                )
            }
        };

        if let Err(e) = db::agent_runs::finish_run(&self.db, &run_id, status, &transcript).await {
            logfire::error!("failed to finish agent run", error = e.to_string(),);
        }

        if status == "failed" {
            return Err(AgentError::ExecutionFailed {
                message: transcript.to_string(),
            });
        }

        Ok((transcript, metadata))
    }

    /// List all agent IDs (running and completed).
    pub async fn list_agent_ids(&self) -> Vec<String> {
        self.handles.lock().await.keys().cloned().collect()
    }

    fn spawn_run(
        &self,
        agent_name: String,
        prompt: String,
        agent_session_id: String,
        run_id: String,
        cancel_token: CancellationToken,
        metadata_slot: Arc<Mutex<Option<RunMetadata>>>,
    ) -> JoinHandle<()> {
        let db = self.db.clone();
        let config = self.config.clone();

        tokio::spawn(async move {
            let result = run_agent(
                &db,
                &config,
                &agent_name,
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
                        agent_name = agent_name.clone(),
                        error = e.to_string(),
                    );
                    ("failed", format!("Agent error: {e}"))
                }
            };

            if let Err(e) = db::agent_runs::finish_run(&db, &run_id, status, &transcript).await {
                logfire::error!("failed to finish agent run", error = e.to_string(),);
            }
        })
    }

    fn spawn_continue(
        &self,
        agent_name: String,
        prompt: String,
        agent_session_id: String,
        run_id: String,
        cancel_token: CancellationToken,
        metadata_slot: Arc<Mutex<Option<RunMetadata>>>,
    ) -> JoinHandle<()> {
        let db = self.db.clone();
        let config = self.config.clone();

        tokio::spawn(async move {
            let result = continue_agent_inner(
                &db,
                &config,
                &agent_name,
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
                        agent_name = agent_name.clone(),
                        error = e.to_string(),
                    );
                    ("failed", format!("Agent error: {e}"))
                }
            };

            if let Err(e) = db::agent_runs::finish_run(&db, &run_id, status, &transcript).await {
                logfire::error!("failed to finish agent run", error = e.to_string(),);
            }
        })
    }

    async fn get_run_transcript(&self, run_id: &str) -> Option<String> {
        let logs = db::agent_runs::list_runs(&self.db, None, 100).await.ok()?;
        logs.into_iter()
            .find(|log| log.id == run_id)
            .and_then(|log| log.transcript)
    }
}

/// Execute a Lua-defined agent in a fresh session.
#[tracing::instrument(name = "run agent", skip_all, fields(
    gen_ai.agent.name = %agent_name,
    gen_ai.agent.id = ?agent_session_id,
    gen_ai.operation.name = "invoke_agent",
))]
async fn run_agent(
    db: &GhostDb,
    config: &Config,
    agent_name: &str,
    prompt: &str,
    agent_session_id: &str,
    cancel_token: &CancellationToken,
) -> Result<(String, RunMetadata), AgentError> {
    let (agent_config, script_host) = load_agent_with_host(&config.workspace, agent_name)?;
    let script_host = Arc::new(script_host);

    let mut system_prompt = agent_config.system_prompt.clone().unwrap_or_default();

    // Wire build_context hook
    if agent_config.has_build_context {
        let ctx = AgentContext {
            db: db.clone(),
            workspace: config.workspace.clone(),
            agent_slug: agent_name.to_string(),
            session_id: agent_session_id.to_string(),
            trigger_session_id: None,
            trigger_agent_name: None,
        };
        match script_host.call_build_context(ctx) {
            Ok(Some(extra)) => {
                system_prompt = format!("{extra}\n\n{system_prompt}");
            }
            Ok(None) => {}
            Err(e) => {
                logfire::warn!(
                    "build_context hook error, using original prompt",
                    agent_name = agent_name.to_string(),
                    error = e.to_string(),
                );
            }
        }
    }

    let provider = provider_for_alias(config, agent_config.model.as_deref())?;

    let mut tools = agent_config.tools.clone();
    if !agent_config.skills.is_empty() && !tools.iter().any(|t| t == "read_file") {
        tools.push("read_file".to_string());
    }
    let mut tool_manager = ToolManager::for_agent(&tools);
    for custom_tool in build_custom_tools(&agent_config, &script_host) {
        tool_manager.register(custom_tool);
    }

    let session_chat = SessionChat::new(db.clone(), provider, tool_manager, config.clone())
        .with_max_tool_iterations(agent_config.max_iterations);

    let result = tokio::select! {
        res = session_chat.run_agent(
            agent_session_id,
            prompt,
            system_prompt,
            &agent_config,
            &script_host,
            None,
        ) => res?,
        () = cancel_token.cancelled() => {
            logfire::info!("agent cancelled", agent_name = agent_name.to_string());
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

    // Wire post_completion hook
    if agent_config.has_post_completion {
        let ctx = AgentContext {
            db: db.clone(),
            workspace: config.workspace.clone(),
            agent_slug: agent_name.to_string(),
            session_id: agent_session_id.to_string(),
            trigger_session_id: None,
            trigger_agent_name: None,
        };
        if let Err(e) = script_host.call_post_completion(ctx) {
            logfire::warn!(
                "post_completion hook error",
                agent_name = agent_name.to_string(),
                error = e.to_string(),
            );
        }
    }

    Ok((result.0.message, result.1))
}

/// Continue a Lua-defined agent with full history from an existing session.
#[tracing::instrument(name = "continue agent", skip_all, fields(
    gen_ai.agent.name = %agent_name,
    gen_ai.agent.id = ?agent_session_id,
    gen_ai.operation.name = "invoke_agent",
))]
async fn continue_agent_inner(
    db: &GhostDb,
    config: &Config,
    agent_name: &str,
    prompt: &str,
    agent_session_id: &str,
    cancel_token: &CancellationToken,
) -> Result<(String, RunMetadata), AgentError> {
    let (agent_config, script_host) = load_agent_with_host(&config.workspace, agent_name)?;
    let script_host = Arc::new(script_host);

    let mut system_prompt = agent_config.system_prompt.clone().unwrap_or_default();

    // Wire build_context hook
    if agent_config.has_build_context {
        let ctx = AgentContext {
            db: db.clone(),
            workspace: config.workspace.clone(),
            agent_slug: agent_name.to_string(),
            session_id: agent_session_id.to_string(),
            trigger_session_id: None,
            trigger_agent_name: None,
        };
        match script_host.call_build_context(ctx) {
            Ok(Some(extra)) => {
                system_prompt = format!("{extra}\n\n{system_prompt}");
            }
            Ok(None) => {}
            Err(e) => {
                logfire::warn!(
                    "build_context hook error, using original prompt",
                    agent_name = agent_name.to_string(),
                    error = e.to_string(),
                );
            }
        }
    }

    let provider = provider_for_alias(config, agent_config.model.as_deref())?;

    let mut tools = agent_config.tools.clone();
    if !agent_config.skills.is_empty() && !tools.iter().any(|t| t == "read_file") {
        tools.push("read_file".to_string());
    }
    let mut tool_manager = ToolManager::for_agent(&tools);
    for custom_tool in build_custom_tools(&agent_config, &script_host) {
        tool_manager.register(custom_tool);
    }

    let session_chat = SessionChat::new(db.clone(), provider, tool_manager, config.clone())
        .with_max_tool_iterations(agent_config.max_iterations);

    let result = tokio::select! {
        res = session_chat.continue_agent(
            agent_session_id,
            prompt,
            system_prompt,
            &agent_config,
            &script_host,
            None,
        ) => res?,
        () = cancel_token.cancelled() => {
            logfire::info!("agent continuation cancelled", agent_name = agent_name.to_string());
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

    // Wire post_completion hook
    if agent_config.has_post_completion {
        let ctx = AgentContext {
            db: db.clone(),
            workspace: config.workspace.clone(),
            agent_slug: agent_name.to_string(),
            session_id: agent_session_id.to_string(),
            trigger_session_id: None,
            trigger_agent_name: None,
        };
        if let Err(e) = script_host.call_post_completion(ctx) {
            logfire::warn!(
                "post_completion hook error",
                agent_name = agent_name.to_string(),
                error = e.to_string(),
            );
        }
    }

    Ok((result.0.message, result.1))
}

/// Parse an agent_id string (e.g. "session:abc123") into a bare ID string.
fn parse_agent_session_id(agent_id: &str) -> Result<String, AgentError> {
    if let Some((_table, id)) = agent_id.split_once(':') {
        if id.is_empty() {
            return Err(AgentError::AgentSessionNotFound {
                agent_session_id: agent_id.to_string(),
            });
        }
        Ok(id.to_string())
    } else if !agent_id.is_empty() {
        Ok(agent_id.to_string())
    } else {
        Err(AgentError::AgentSessionNotFound {
            agent_session_id: agent_id.to_string(),
        })
    }
}

impl std::fmt::Debug for AgentHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentHandle")
            .field("agent_id", &self.agent_id)
            .field("agent_name", &self.agent_name)
            .field("finished", &self.join_handle.is_finished())
            .finish()
    }
}
