use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::chat::{RunMetadata, SessionChat};
use crate::config::{CompactionConfig, Config};
use crate::db;
use crate::db::GhostDb;
use crate::providers::provider_for_alias;
use crate::scripting::AgentContext;
use crate::scripting::LuaMessage;
use crate::scripting::ScriptHost;
use crate::scripting::SpawnRequest;
use crate::scripting::build_custom_tools;
use crate::scripting::types::{AgentCompactionOverrides, AgentConfig, BuildResult};
use crate::tools::ToolManager;

use super::error::AgentError;
use super::loader::load_agent_with_host;

/// Maximum spawn depth. Root = depth 0, child = depth 1, depth >= 2 → drop.
const MAX_SPAWN_DEPTH: u32 = 2;

/// Result of a synchronous agent execution.
#[derive(Debug, Clone)]
pub struct AgentResult {
    pub session_id: String,
    pub findings: String,
    pub metadata: RunMetadata,
    pub spawns: Vec<SpawnRequest>,
}

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

/// Manages agent execution (sync and background).
#[derive(Debug, Clone)]
pub struct AgentRunner {
    db: GhostDb,
    config: Config,
    handles: Arc<Mutex<HashMap<String, AgentHandle>>>,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

impl AgentRunner {
    #[must_use]
    pub fn new(db: GhostDb, config: Config) -> Self {
        Self {
            db,
            config,
            handles: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    // --- Sync (caller awaits, caller decides about spawns) ----------------

    /// Run an agent synchronously with a prompt string.
    pub async fn run(
        &self,
        agent_name: &str,
        prompt: &str,
        parent_session_id: Option<&str>,
    ) -> Result<AgentResult, AgentError> {
        self.run_with_args(agent_name, prompt_args(prompt), parent_session_id)
            .await
    }

    /// Run an agent synchronously with arbitrary args.
    pub async fn run_with_args(
        &self,
        agent_name: &str,
        args: HashMap<String, String>,
        parent_session_id: Option<&str>,
    ) -> Result<AgentResult, AgentError> {
        let agent_session_id = db::sessions::create_agent_session(&self.db).await?;

        let run_id = db::agent_runs::create_agent_run(
            &self.db,
            agent_name,
            parent_session_id,
            &agent_session_id,
        )
        .await?;

        let cancel_token = CancellationToken::new();

        let result = execute_agent(
            &self.db,
            &self.config,
            agent_name,
            args,
            &agent_session_id,
            &cancel_token,
        )
        .await;

        finish_run_and_return(&self.db, &run_id, agent_name, &agent_session_id, result).await
    }

    /// Resume an existing agent session with a new prompt.
    pub async fn resume(
        &self,
        session_id: &str,
        prompt: &str,
        agent_name: &str,
    ) -> Result<AgentResult, AgentError> {
        let run_id =
            db::agent_runs::create_agent_run(&self.db, agent_name, None, session_id).await?;

        let cancel_token = CancellationToken::new();

        let result = execute_resume(
            &self.db,
            &self.config,
            agent_name,
            prompt,
            session_id,
            &cancel_token,
        )
        .await;

        finish_run_and_return(&self.db, &run_id, agent_name, session_id, result).await
    }

    // --- Background (tracked handles, processes spawns with depth limit) --

    /// Start an agent in the background. Returns the agent_id.
    #[tracing::instrument(name = "start agent", skip_all, fields(
        gen_ai.agent.name = %agent_name,
    ))]
    pub async fn run_in_background(
        &self,
        agent_name: &str,
        prompt: &str,
        parent_session_id: Option<&str>,
    ) -> Result<String, AgentError> {
        let agent_dir = self.config.workspace.join("agents").join(agent_name);
        if !agent_dir.join("agent.lua").exists() {
            return Err(AgentError::NotFound {
                name: agent_name.to_string(),
            });
        }

        let agent_session_id = db::sessions::create_agent_session(&self.db).await?;
        let agent_id = agent_session_id.clone();

        let run_id = db::agent_runs::create_agent_run(
            &self.db,
            agent_name,
            parent_session_id,
            &agent_session_id,
        )
        .await?;

        let cancel_token = CancellationToken::new();
        let metadata_slot: Arc<Mutex<Option<RunMetadata>>> = Arc::new(Mutex::new(None));

        let join_handle = spawn_background_run(
            BackgroundTask {
                db: self.db.clone(),
                config: self.config.clone(),
                agent_name: agent_name.to_string(),
                agent_session_id: agent_session_id.clone(),
                run_id: run_id.clone(),
                cancel_token: cancel_token.clone(),
                metadata_slot: Arc::clone(&metadata_slot),
                depth: 0,
            },
            prompt_args(prompt),
        );

        let handle = AgentHandle {
            agent_id: agent_id.clone(),
            agent_name: agent_name.to_string(),
            parent_session_id: parent_session_id.map(|s| s.to_string()),
            agent_session_id,
            run_id,
            join_handle,
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

    /// Resume an agent session in the background. Returns the agent name.
    #[tracing::instrument(name = "resume agent bg", skip_all, fields(agent_id = agent_id))]
    pub async fn resume_in_background(
        &self,
        agent_id: &str,
        prompt: &str,
        parent_session_id: Option<&str>,
    ) -> Result<String, AgentError> {
        let agent_session_id = parse_agent_session_id(agent_id)?;

        let agent_name = db::agent_runs::get_agent_name_for_session(&self.db, &agent_session_id)
            .await?
            .ok_or_else(|| AgentError::AgentSessionNotFound {
                agent_session_id: agent_id.to_string(),
            })?;

        let run_id = db::agent_runs::create_agent_run(
            &self.db,
            &agent_name,
            parent_session_id,
            &agent_session_id,
        )
        .await?;

        let cancel_token = CancellationToken::new();
        let metadata_slot: Arc<Mutex<Option<RunMetadata>>> = Arc::new(Mutex::new(None));

        let join_handle = spawn_background_resume(
            BackgroundTask {
                db: self.db.clone(),
                config: self.config.clone(),
                agent_name: agent_name.clone(),
                agent_session_id: agent_session_id.clone(),
                run_id: run_id.clone(),
                cancel_token: cancel_token.clone(),
                metadata_slot: Arc::clone(&metadata_slot),
                depth: 0,
            },
            prompt.to_string(),
        );

        let handle = AgentHandle {
            agent_id: agent_id.to_string(),
            agent_name: agent_name.clone(),
            parent_session_id: parent_session_id.map(|s| s.to_string()),
            agent_session_id,
            run_id,
            join_handle,
            cancel_token,
            metadata: metadata_slot,
        };
        self.handles
            .lock()
            .await
            .insert(agent_id.to_string(), handle);

        logfire::info!(
            "agent resumed (background)",
            agent_name = agent_name.clone(),
            agent_id = agent_id.to_string(),
        );

        Ok(agent_name)
    }

    // --- Spawn helper -----------------------------------------------------

    /// Fire child agents from spawn requests with depth=0.
    pub fn spawn_children(&self, result: &mut AgentResult) {
        let spawns = std::mem::take(&mut result.spawns);
        spawn_children_inner(spawns, &self.db, &self.config, &result.session_id, 0);
    }

    // --- Handle management ------------------------------------------------

    pub async fn status(&self, agent_id: &str) -> Result<AgentStatus, AgentError> {
        let handles = self.handles.lock().await;
        let handle = handles
            .get(agent_id)
            .ok_or_else(|| AgentError::AgentNotFound {
                agent_id: agent_id.to_string(),
            })?;

        let is_finished = handle.join_handle.is_finished();

        let message_count =
            db::sessions::count_messages_for_session(&self.db, &handle.agent_session_id)
                .await
                .unwrap_or(0);

        let todo_summary = db::sessions::get_session_todo_list(&self.db, &handle.agent_session_id)
            .await
            .ok()
            .flatten()
            .map(|items| crate::tools::format_todo_list(&items));

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

    pub async fn stop(&self, agent_id: &str) -> Result<AgentStatus, AgentError> {
        let handles = self.handles.lock().await;
        let handle = handles
            .get(agent_id)
            .ok_or_else(|| AgentError::AgentNotFound {
                agent_id: agent_id.to_string(),
            })?;

        handle.cancel_token.cancel();
        let agent_name = handle.agent_name.clone();
        let agent_session_id = handle.agent_session_id.clone();
        let run_id = handle.run_id.clone();
        drop(handles);

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

    pub async fn list_agent_ids(&self) -> Vec<String> {
        self.handles.lock().await.keys().cloned().collect()
    }

    async fn get_run_transcript(&self, run_id: &str) -> Option<String> {
        db::agent_runs::get_run(&self.db, run_id)
            .await
            .ok()
            .flatten()
            .and_then(|run| run.transcript)
    }
}

// ---------------------------------------------------------------------------
// Internal execution functions
// ---------------------------------------------------------------------------

/// Shared setup for agent runs.
struct AgentSetup {
    config: AgentConfig,
    build_result: BuildResult,
    script_host: Arc<ScriptHost>,
    session_chat: SessionChat,
    /// Spawn requests accumulated by tool handlers via `ctx:spawn_agent`.
    /// Kept separate from post_completion so both sources are collected.
    build_spawn_requests: Arc<std::sync::Mutex<Vec<SpawnRequest>>>,
}

async fn setup_agent(
    db: &GhostDb,
    config: &Config,
    agent_name: &str,
    args: HashMap<String, String>,
    agent_session_id: &str,
) -> Result<AgentSetup, AgentError> {
    let (agent_config, script_host) = load_agent_with_host(&config.workspace, agent_name)?;
    let script_host = Arc::new(script_host);

    let ctx = AgentContext::new(
        db.clone(),
        config.workspace.clone(),
        agent_name.to_string(),
        agent_session_id.to_string(),
    );
    // Keep a handle so spawn_requests from tool handlers aren't lost
    // when post_completion creates a fresh ctx.
    let build_spawn_requests = ctx.spawn_requests.clone();
    let build_result =
        script_host
            .call_build(ctx, args)
            .await
            .map_err(|e| AgentError::ScriptError {
                agent: agent_name.to_string(),
                message: format!("build hook failed: {e}"),
            })?;

    let provider = provider_for_alias(config, agent_config.model.as_deref())?;

    let mut tools = agent_config.tools.clone();
    if !agent_config.skills.is_empty() && !tools.iter().any(|t| t == "read_file") {
        tools.push("read_file".to_string());
    }
    let mut tool_manager = ToolManager::for_agent(&tools);
    for custom_tool in build_custom_tools(&agent_config, &script_host) {
        tool_manager.register(custom_tool);
    }

    let agent_compaction =
        build_agent_compaction_config(&config.compaction, agent_config.compaction.as_ref());

    let session_chat = SessionChat::new(db.clone(), provider, tool_manager, config.clone())
        .with_max_tool_iterations(agent_config.max_iterations)
        .with_compaction_config(agent_compaction);

    Ok(AgentSetup {
        config: agent_config,
        build_result,
        script_host,
        session_chat,
        build_spawn_requests,
    })
}

/// Setup for resuming an existing session.
struct ResumeSetup {
    config: AgentConfig,
    system_prompt: String,
    messages: Vec<LuaMessage>,
    db_message_count: usize,
    script_host: Arc<ScriptHost>,
    session_chat: SessionChat,
    build_spawn_requests: Arc<std::sync::Mutex<Vec<SpawnRequest>>>,
}

async fn setup_resume(
    db: &GhostDb,
    config: &Config,
    agent_name: &str,
    prompt: &str,
    session_id: &str,
) -> Result<ResumeSetup, AgentError> {
    // Get config + system prompt from build()
    let setup = setup_agent(db, config, agent_name, prompt_args(prompt), session_id).await?;

    let db_messages = db::sessions::list_messages_by_session(db, session_id)
        .await
        .unwrap_or_default();

    let db_message_count = db_messages.len();
    let mut messages: Vec<LuaMessage> = db_messages
        .iter()
        .map(|m| LuaMessage {
            role: m.role.clone(),
            content: m.content.clone(),
        })
        .collect();

    // Populate ctx fields for on_resume hook
    let system_prompt = setup.build_result.system_prompt.clone();

    if setup.config.has_on_resume {
        let ctx = AgentContext::new(
            db.clone(),
            config.workspace.clone(),
            agent_name.to_string(),
            session_id.to_string(),
        );
        // Pre-populate editable fields
        *ctx.system_prompt.lock().expect("lock") = Some(system_prompt.clone());
        *ctx.resume_messages.lock().expect("lock") = Some(messages.clone());

        setup
            .script_host
            .call_on_resume(ctx.clone(), prompt)
            .await
            .map_err(|e| AgentError::ScriptError {
                agent: agent_name.to_string(),
                message: format!("on_resume hook failed: {e}"),
            })?;

        // Read back edited fields
        let edited_prompt = ctx
            .system_prompt
            .lock()
            .expect("lock")
            .clone()
            .unwrap_or(system_prompt);
        let edited_messages = ctx
            .resume_messages
            .lock()
            .expect("lock")
            .take()
            .unwrap_or(messages);

        return Ok(ResumeSetup {
            config: setup.config,
            system_prompt: edited_prompt,
            messages: edited_messages,
            db_message_count,
            script_host: setup.script_host,
            session_chat: setup.session_chat,
            build_spawn_requests: setup.build_spawn_requests,
        });
    }

    // Default: append prompt as user message
    messages.push(LuaMessage {
        role: "user".to_string(),
        content: prompt.to_string(),
    });

    Ok(ResumeSetup {
        config: setup.config,
        system_prompt,
        messages,
        db_message_count,
        script_host: setup.script_host,
        session_chat: setup.session_chat,
        build_spawn_requests: setup.build_spawn_requests,
    })
}

/// Run the post_completion hook if present. Returns any spawn requests.
async fn run_post_completion(
    agent_config: &AgentConfig,
    script_host: &ScriptHost,
    db: &GhostDb,
    config: &Config,
    agent_name: &str,
    agent_session_id: &str,
) -> Vec<SpawnRequest> {
    if agent_config.has_post_completion {
        let ctx = AgentContext::new(
            db.clone(),
            config.workspace.clone(),
            agent_name.to_string(),
            agent_session_id.to_string(),
        );
        let spawn_requests = ctx.spawn_requests.clone();
        if let Err(e) = script_host.call_post_completion(ctx).await {
            logfire::warn!(
                "post_completion hook error",
                agent_name = agent_name.to_string(),
                error = e.to_string(),
            );
        }
        std::mem::take(&mut *spawn_requests.lock().expect("spawn_requests lock"))
    } else {
        Vec::new()
    }
}

/// Execute a fresh agent run. Returns `AgentResult`.
#[tracing::instrument(name = "execute agent", skip_all, fields(
    gen_ai.agent.name = %agent_name,
    gen_ai.agent.id = ?agent_session_id,
    gen_ai.operation.name = "invoke_agent",
))]
async fn execute_agent(
    db: &GhostDb,
    config: &Config,
    agent_name: &str,
    args: HashMap<String, String>,
    agent_session_id: &str,
    cancel_token: &CancellationToken,
) -> Result<AgentResult, AgentError> {
    let setup = setup_agent(db, config, agent_name, args, agent_session_id).await?;

    let result = tokio::select! {
        res = setup.session_chat.run_agent(
            agent_session_id,
            setup.build_result,
            &setup.config,
            &setup.script_host,
            None,
        ) => res?,
        () = cancel_token.cancelled() => {
            logfire::info!("agent cancelled", agent_name = agent_name.to_string());
            return Ok(AgentResult {
                session_id: agent_session_id.to_string(),
                findings: last_assistant_message(db, agent_session_id).await,
                metadata: RunMetadata::default(),
                spawns: Vec::new(),
            });
        }
    };

    // Collect spawn requests from tool handlers (accumulated on the build ctx)
    let mut spawns = std::mem::take(
        &mut *setup
            .build_spawn_requests
            .lock()
            .expect("build_spawn_requests lock"),
    );

    // Also collect spawns from the post_completion hook
    spawns.extend(
        run_post_completion(
            &setup.config,
            &setup.script_host,
            db,
            config,
            agent_name,
            agent_session_id,
        )
        .await,
    );
    Ok(AgentResult {
        session_id: agent_session_id.to_string(),
        findings: result.0.message,
        metadata: result.1,
        spawns,
    })
}

/// Resume an existing agent session. Returns `AgentResult`.
#[tracing::instrument(name = "execute resume", skip_all, fields(
    gen_ai.agent.name = %agent_name,
    gen_ai.agent.id = ?session_id,
    gen_ai.operation.name = "invoke_agent",
))]
async fn execute_resume(
    db: &GhostDb,
    config: &Config,
    agent_name: &str,
    prompt: &str,
    session_id: &str,
    cancel_token: &CancellationToken,
) -> Result<AgentResult, AgentError> {
    let resume = setup_resume(db, config, agent_name, prompt, session_id).await?;

    let result = tokio::select! {
        res = resume.session_chat.run_agent_with_history(
            session_id,
            resume.system_prompt,
            &resume.messages,
            resume.db_message_count,
            &resume.config,
            &resume.script_host,
            None,
        ) => res?,
        () = cancel_token.cancelled() => {
            logfire::info!("agent resume cancelled", agent_name = agent_name.to_string());
            return Ok(AgentResult {
                session_id: session_id.to_string(),
                findings: last_assistant_message(db, session_id).await,
                metadata: RunMetadata::default(),
                spawns: Vec::new(),
            });
        }
    };

    // Collect spawn requests from tool handlers (accumulated on the build ctx)
    let mut spawns = std::mem::take(
        &mut *resume
            .build_spawn_requests
            .lock()
            .expect("build_spawn_requests lock"),
    );

    // Also collect spawns from the post_completion hook
    spawns.extend(
        run_post_completion(
            &resume.config,
            &resume.script_host,
            db,
            config,
            agent_name,
            session_id,
        )
        .await,
    );
    Ok(AgentResult {
        session_id: session_id.to_string(),
        findings: result.0.message,
        metadata: result.1,
        spawns,
    })
}

// ---------------------------------------------------------------------------
// Background spawning with depth limit
// ---------------------------------------------------------------------------

/// Shared params for background agent tasks.
struct BackgroundTask {
    db: GhostDb,
    config: Config,
    agent_name: String,
    agent_session_id: String,
    run_id: String,
    cancel_token: CancellationToken,
    metadata_slot: Arc<Mutex<Option<RunMetadata>>>,
    depth: u32,
}

fn spawn_background_run(task: BackgroundTask, args: HashMap<String, String>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let result = execute_agent(
            &task.db,
            &task.config,
            &task.agent_name,
            args,
            &task.agent_session_id,
            &task.cancel_token,
        )
        .await;

        finish_background(task, result).await;
    })
}

fn spawn_background_resume(task: BackgroundTask, prompt: String) -> JoinHandle<()> {
    tokio::spawn(async move {
        let result = execute_resume(
            &task.db,
            &task.config,
            &task.agent_name,
            &prompt,
            &task.agent_session_id,
            &task.cancel_token,
        )
        .await;

        finish_background(task, result).await;
    })
}

async fn finish_background(task: BackgroundTask, result: Result<AgentResult, AgentError>) {
    let (status, transcript) = match result {
        Ok(agent_result) => {
            *task.metadata_slot.lock().await = Some(agent_result.metadata);
            spawn_children_inner(
                agent_result.spawns,
                &task.db,
                &task.config,
                &task.agent_session_id,
                task.depth,
            );
            ("ok", agent_result.findings)
        }
        Err(e) => {
            logfire::error!(
                "agent failed",
                agent_name = task.agent_name.clone(),
                error = e.to_string(),
            );
            let partial = last_assistant_message(&task.db, &task.agent_session_id).await;
            ("failed", partial)
        }
    };

    if let Err(e) = db::agent_runs::finish_run(&task.db, &task.run_id, status, &transcript).await {
        logfire::error!("failed to finish agent run", error = e.to_string());
    }
}

/// Spawn child agents from post_completion, enforcing depth limit.
fn spawn_children_inner(
    requests: Vec<SpawnRequest>,
    db: &GhostDb,
    config: &Config,
    parent_session_id: &str,
    depth: u32,
) {
    let child_depth = depth + 1;
    if child_depth >= MAX_SPAWN_DEPTH {
        if !requests.is_empty() {
            logfire::info!(
                "dropping spawn requests at depth limit",
                count = requests.len(),
                depth = depth,
            );
        }
        return;
    }

    for req in requests {
        let db = db.clone();
        let config = config.clone();
        let parent_id = parent_session_id.to_string();

        tokio::spawn(async move {
            let agent_session_id = match db::sessions::create_agent_session(&db).await {
                Ok(id) => id,
                Err(e) => {
                    logfire::error!(
                        "failed to create session for spawned agent",
                        agent = req.agent.clone(),
                        error = e.to_string(),
                    );
                    return;
                }
            };

            let run_id = match db::agent_runs::create_agent_run(
                &db,
                &req.agent,
                Some(&parent_id),
                &agent_session_id,
            )
            .await
            {
                Ok(id) => id,
                Err(e) => {
                    logfire::error!(
                        "failed to create run for spawned agent",
                        agent = req.agent.clone(),
                        error = e.to_string(),
                    );
                    return;
                }
            };

            let cancel_token = CancellationToken::new();

            logfire::info!(
                "spawning child agent",
                agent = req.agent.clone(),
                parent_session_id = parent_id.clone(),
                depth = child_depth,
            );

            let result = execute_agent(
                &db,
                &config,
                &req.agent,
                req.args,
                &agent_session_id,
                &cancel_token,
            )
            .await;

            let (status, transcript) = match result {
                Ok(agent_result) => {
                    spawn_children_inner(
                        agent_result.spawns,
                        &db,
                        &config,
                        &agent_session_id,
                        child_depth,
                    );
                    ("ok", agent_result.findings)
                }
                Err(e) => {
                    logfire::error!(
                        "spawned agent failed",
                        agent = req.agent.clone(),
                        error = e.to_string(),
                    );
                    let partial = last_assistant_message(&db, &agent_session_id).await;
                    ("failed", partial)
                }
            };

            if let Err(e) = db::agent_runs::finish_run(&db, &run_id, status, &transcript).await {
                logfire::error!("failed to finish spawned agent run", error = e.to_string());
            }
        });
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Finish a run record and convert the result to the public return type.
async fn finish_run_and_return(
    db: &GhostDb,
    run_id: &str,
    agent_name: &str,
    session_id: &str,
    result: Result<AgentResult, AgentError>,
) -> Result<AgentResult, AgentError> {
    let (status, agent_result) = match result {
        Ok(r) => ("ok", r),
        Err(e) => {
            logfire::error!(
                "agent execution failed",
                agent_name = agent_name.to_string(),
                error = e.to_string(),
            );
            let transcript = format!("Agent error: {e}");
            if let Err(fe) = db::agent_runs::finish_run(db, run_id, "failed", &transcript).await {
                logfire::error!("failed to finish agent run", error = fe.to_string());
            }
            return Err(AgentError::ExecutionFailed {
                message: transcript,
            });
        }
    };

    if let Err(e) = db::agent_runs::finish_run(db, run_id, status, &agent_result.findings).await {
        logfire::error!("failed to finish agent run", error = e.to_string());
    }

    Ok(AgentResult {
        session_id: session_id.to_string(),
        ..agent_result
    })
}

async fn last_assistant_message(db: &GhostDb, session_id: &str) -> String {
    db::sessions::list_messages_by_session(db, session_id)
        .await
        .unwrap_or_default()
        .iter()
        .rev()
        .find(|m| m.role == "assistant" && !m.content.is_empty())
        .map(|m| m.content.clone())
        .unwrap_or_else(|| "Agent was cancelled before producing findings.".to_string())
}

fn prompt_args(prompt: &str) -> HashMap<String, String> {
    HashMap::from([("prompt".into(), prompt.to_string())])
}

/// Build an agent-specific `CompactionConfig` by layering:
/// 1. Agent defaults (keep_window=10, threshold=0.90)
/// 2. Lua overrides from `agent_config.compaction` (any field present wins)
/// 3. Global config `instructions` as fallback if the agent didn't specify any
fn build_agent_compaction_config(
    global: &CompactionConfig,
    overrides: Option<&AgentCompactionOverrides>,
) -> CompactionConfig {
    // Agent defaults differ from chat defaults
    let mut cfg = CompactionConfig {
        threshold: 0.90,
        keep_window: 10,
        mask_preview_chars: 100,
        instructions: global.instructions.clone(),
    };

    if let Some(o) = overrides {
        if let Some(t) = o.threshold {
            cfg.threshold = t;
        }
        if let Some(kw) = o.keep_window {
            cfg.keep_window = kw;
        }
        if let Some(mpc) = o.mask_preview_chars {
            cfg.mask_preview_chars = mpc;
        }
        if o.instructions.is_some() {
            cfg.instructions = o.instructions.clone();
        }
    }

    cfg
}

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

#[cfg(test)]
mod tests {
    use super::*;

    fn global_config() -> CompactionConfig {
        CompactionConfig {
            threshold: 0.90,
            keep_window: 20,
            mask_preview_chars: 100,
            instructions: None,
        }
    }

    #[test]
    fn agent_compaction_defaults_without_overrides() {
        let cfg = build_agent_compaction_config(&global_config(), None);

        assert_eq!(cfg.keep_window, 10, "agent default differs from chat");
        assert_eq!(cfg.threshold, 0.90);
        assert_eq!(cfg.mask_preview_chars, 100);
        assert!(cfg.instructions.is_none());
    }

    #[test]
    fn agent_compaction_lua_overrides_win() {
        let overrides = AgentCompactionOverrides {
            keep_window: Some(8),
            threshold: Some(0.70),
            mask_preview_chars: None,
            instructions: Some("Keep all URLs.".into()),
        };

        let cfg = build_agent_compaction_config(&global_config(), Some(&overrides));

        assert_eq!(cfg.keep_window, 8);
        assert_eq!(cfg.threshold, 0.70);
        assert_eq!(cfg.mask_preview_chars, 100, "unset fields keep defaults");
        assert_eq!(cfg.instructions.as_deref(), Some("Keep all URLs."));
    }

    #[test]
    fn agent_compaction_inherits_global_instructions() {
        let mut global = global_config();
        global.instructions = Some("Global hint.".into());

        let cfg = build_agent_compaction_config(&global, None);

        assert_eq!(
            cfg.instructions.as_deref(),
            Some("Global hint."),
            "agent inherits global instructions when no override"
        );
    }

    #[test]
    fn agent_compaction_lua_instructions_override_global() {
        let mut global = global_config();
        global.instructions = Some("Global hint.".into());

        let overrides = AgentCompactionOverrides {
            instructions: Some("Agent-specific.".into()),
            ..Default::default()
        };

        let cfg = build_agent_compaction_config(&global, Some(&overrides));

        assert_eq!(
            cfg.instructions.as_deref(),
            Some("Agent-specific."),
            "Lua instructions replace global"
        );
    }
}
