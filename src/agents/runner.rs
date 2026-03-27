use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::Instrument;

use crate::chat::{RunMetadata, SessionChat, ToolLoopContext};
use crate::config::{CompactionConfig, Config, SharedConfig, SharedConfigExt};
use crate::db;
use crate::db::GhostDb;
use crate::providers::{provider_for_chain, provider_for_model_ref};
use crate::scripting::AgentContext;
use crate::scripting::LuaMessage;
use crate::scripting::ScriptHost;
use crate::scripting::SpawnRequest;
use crate::scripting::build_custom_tools;
use crate::scripting::types::{AgentCompactionOverrides, AgentConfig, BuildResult};
use crate::tools::ToolManager;

use super::error::AgentError;
use super::loader::load_agent_with_host;

use crate::constants::MAX_SPAWN_DEPTH;

/// Brief delay before reading agent results to let DB writes settle.
const RESULT_POLL_DELAY: std::time::Duration = std::time::Duration::from_millis(100);

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
    agent_session_id: String,
    run_id: String,
    join_handle: JoinHandle<()>,
    cancel_token: CancellationToken,
    metadata: Arc<Mutex<Option<RunMetadata>>>,
    _cwd: Option<PathBuf>,
}

/// Manages agent execution (sync and background).
#[derive(Debug, Clone)]
pub struct AgentRunner {
    db: GhostDb,
    config: SharedConfig,
    handles: Arc<Mutex<HashMap<String, AgentHandle>>>,
    event_tx: Option<crate::events::SessionEventSender>,
    active_count: Arc<AtomicUsize>,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

impl AgentRunner {
    #[must_use]
    pub fn new(
        db: GhostDb,
        config: SharedConfig,
        event_tx: Option<crate::events::SessionEventSender>,
    ) -> Self {
        Self {
            db,
            config,
            handles: Arc::new(Mutex::new(HashMap::new())),
            event_tx,
            active_count: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Number of currently running background agent tasks.
    pub fn active_count(&self) -> usize {
        self.active_count.load(Ordering::Relaxed)
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
        let config = self.config.current();

        let result = execute_agent(
            &self.db,
            config,
            agent_name,
            args,
            &agent_session_id,
            &cancel_token,
            parent_session_id,
            None,
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
        let config = self.config.current();

        let result = execute_resume(
            &self.db,
            config,
            agent_name,
            prompt,
            session_id,
            &cancel_token,
            None,
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
        cwd: Option<PathBuf>,
    ) -> Result<String, AgentError> {
        // Verify agent exists before spawning background task.
        let config = self.config.current();
        if super::loader::resolve_agent_dir(&config.workspace, agent_name).is_none() {
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
                config,
                agent_name: agent_name.to_string(),
                agent_session_id: agent_session_id.clone(),
                parent_session_id: parent_session_id.map(|s| s.to_string()),
                run_id: run_id.clone(),
                cancel_token: cancel_token.clone(),
                metadata_slot: Arc::clone(&metadata_slot),
                depth: 0,
                cwd: cwd.clone(),
                event_tx: self.event_tx.clone(),
                handles: Arc::clone(&self.handles),
                active_count: Arc::clone(&self.active_count),
            },
            prompt_args(prompt),
        );

        let handle = AgentHandle {
            agent_id: agent_id.clone(),
            agent_name: agent_name.to_string(),
            agent_session_id,
            run_id,
            join_handle,
            cancel_token,
            metadata: metadata_slot,
            _cwd: cwd,
        };
        self.handles.lock().await.insert(agent_id.clone(), handle);

        tracing::info!(
            agent_name = agent_name.to_string(),
            agent_id = agent_id.clone(),
            "agent started",
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
        cwd: Option<PathBuf>,
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
                config: self.config.current(),
                agent_name: agent_name.clone(),
                agent_session_id: agent_session_id.clone(),
                parent_session_id: parent_session_id.map(|s| s.to_string()),
                run_id: run_id.clone(),
                cancel_token: cancel_token.clone(),
                metadata_slot: Arc::clone(&metadata_slot),
                depth: 0,
                cwd: cwd.clone(),
                event_tx: self.event_tx.clone(),
                handles: Arc::clone(&self.handles),
                active_count: Arc::clone(&self.active_count),
            },
            prompt.to_string(),
        );

        let handle = AgentHandle {
            agent_id: agent_id.to_string(),
            agent_name: agent_name.clone(),
            agent_session_id,
            run_id,
            join_handle,
            cancel_token,
            metadata: metadata_slot,
            _cwd: cwd,
        };
        self.handles
            .lock()
            .await
            .insert(agent_id.to_string(), handle);

        tracing::info!(
            agent_name = agent_name.clone(),
            agent_id = agent_id.to_string(),
            "agent resumed (background)",
        );

        Ok(agent_name)
    }

    // --- Spawn helper -----------------------------------------------------

    /// Fire child agents from spawn requests with depth=0.
    pub fn spawn_children(&self, result: &mut AgentResult) {
        let spawns = std::mem::take(&mut result.spawns);
        let config = self.config.current();
        spawn_children_inner(spawns, &self.db, config, &result.session_id, 0);
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

        tokio::time::sleep(RESULT_POLL_DELAY).await;

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

        tracing::info!(
            agent_name = agent_name.clone(),
            agent_id = agent_id_owned.clone(),
            "agent stopped",
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
    config: Arc<Config>,
    agent_name: &str,
    args: HashMap<String, String>,
    agent_session_id: &str,
    parent_session_id: Option<&str>,
    cwd: Option<&PathBuf>,
) -> Result<AgentSetup, AgentError> {
    let (agent_config, script_host) = load_agent_with_host(&config.workspace, agent_name)?;
    let script_host = Arc::new(script_host);

    let mut ctx = AgentContext::new(
        db.clone(),
        config.workspace.clone(),
        agent_name.to_string(),
        agent_session_id.to_string(),
    );
    ctx.trigger_session_id = parent_session_id.map(String::from);
    ctx = ctx.with_tool_support(Arc::clone(&config), Arc::new(ToolManager::all_available()));
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

    let provider = match &agent_config.model {
        Some(model_ref) => provider_for_model_ref(&config, model_ref)?,
        None => provider_for_chain(&config, &config.models.default_chain)?,
    };

    let mut tools = agent_config.tools.clone();
    if !agent_config.skills.is_empty() && !tools.iter().any(|t| t == "file_read") {
        tools.push("file_read".to_string());
    }
    let mut tool_manager = ToolManager::for_agent(&tools);
    for custom_tool in build_custom_tools(&agent_config, &script_host) {
        tool_manager.register(custom_tool);
    }

    let agent_compaction =
        build_agent_compaction_config(&config.compaction, agent_config.compaction.as_ref());

    // One-shot channel: agent runs use a fixed config snapshot, not hot-reload.
    let (_, cfg_rx) = tokio::sync::watch::channel(Arc::clone(&config));
    let mut session_chat = SessionChat::new(db.clone(), provider, tool_manager, cfg_rx)
        .with_max_tool_iterations(agent_config.max_iterations)
        .with_compaction_config(agent_compaction);

    if let Some(cwd) = cwd {
        session_chat = session_chat.with_cwd_override(cwd.clone());
    }

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
    config: Arc<Config>,
    agent_name: &str,
    prompt: &str,
    session_id: &str,
    cwd: Option<&PathBuf>,
) -> Result<ResumeSetup, AgentError> {
    // Get config + system prompt from build()
    let setup = setup_agent(
        db,
        Arc::clone(&config),
        agent_name,
        prompt_args(prompt),
        session_id,
        None,
        cwd,
    )
    .await?;

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
        )
        .with_tool_support(Arc::clone(&config), Arc::new(ToolManager::all_available()));
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
    config: &Arc<Config>,
    agent_name: &str,
    agent_session_id: &str,
    parent_session_id: Option<&str>,
) -> Vec<SpawnRequest> {
    if agent_config.has_post_completion {
        let mut ctx = AgentContext::new(
            db.clone(),
            config.workspace.clone(),
            agent_name.to_string(),
            agent_session_id.to_string(),
        )
        .with_tool_support(Arc::clone(config), Arc::new(ToolManager::all_available()));
        ctx.trigger_session_id = parent_session_id.map(String::from);
        let spawn_requests = ctx.spawn_requests.clone();
        if let Err(e) = script_host.call_post_completion(ctx).await {
            tracing::warn!(
                agent_name = agent_name.to_string(),
                error = e.to_string(),
                "post_completion hook error",
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
    config: Arc<Config>,
    agent_name: &str,
    args: HashMap<String, String>,
    agent_session_id: &str,
    cancel_token: &CancellationToken,
    parent_session_id: Option<&str>,
    cwd: Option<&PathBuf>,
) -> Result<AgentResult, AgentError> {
    let setup = setup_agent(
        db,
        Arc::clone(&config),
        agent_name,
        args,
        agent_session_id,
        parent_session_id,
        cwd,
    )
    .await?;

    let result = tokio::select! {
        res = setup.session_chat.run_agent(
            agent_session_id,
            setup.build_result,
            &setup.config,
            &setup.script_host,
            None,
            None,
        ) => res?,
        () = cancel_token.cancelled() => {
            tracing::info!(agent_name = agent_name.to_string(), "agent cancelled");
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
            &config,
            agent_name,
            agent_session_id,
            parent_session_id,
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
    config: Arc<Config>,
    agent_name: &str,
    prompt: &str,
    session_id: &str,
    cancel_token: &CancellationToken,
    cwd: Option<&PathBuf>,
) -> Result<AgentResult, AgentError> {
    let resume = setup_resume(db, Arc::clone(&config), agent_name, prompt, session_id, cwd).await?;

    let result = tokio::select! {
        res = resume.session_chat.run_agent_with_history(
            session_id,
            resume.system_prompt,
            &resume.messages,
            resume.db_message_count,
            &resume.config,
            &resume.script_host,
            ToolLoopContext {
                event_tx: None,
                interrupt_rx: None,
                channel_id: None,
            },
        ) => res?,
        () = cancel_token.cancelled() => {
            tracing::info!(agent_name = agent_name.to_string(), "agent resume cancelled");
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

    // Also collect spawns from the post_completion hook (resume has no parent)
    spawns.extend(
        run_post_completion(
            &resume.config,
            &resume.script_host,
            db,
            &config,
            agent_name,
            session_id,
            None,
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
    config: Arc<Config>,
    agent_name: String,
    agent_session_id: String,
    parent_session_id: Option<String>,
    run_id: String,
    cancel_token: CancellationToken,
    metadata_slot: Arc<Mutex<Option<RunMetadata>>>,
    depth: u32,
    cwd: Option<PathBuf>,
    event_tx: Option<crate::events::SessionEventSender>,
    handles: Arc<Mutex<HashMap<String, AgentHandle>>>,
    active_count: Arc<AtomicUsize>,
}

fn spawn_background_run(task: BackgroundTask, args: HashMap<String, String>) -> JoinHandle<()> {
    let span = tracing::Span::current();
    task.active_count.fetch_add(1, Ordering::Relaxed);
    let active_count = Arc::clone(&task.active_count);
    tokio::spawn(
        async move {
            let result = execute_agent(
                &task.db,
                Arc::clone(&task.config),
                &task.agent_name,
                args,
                &task.agent_session_id,
                &task.cancel_token,
                task.parent_session_id.as_deref(),
                task.cwd.as_ref(),
            )
            .await;

            finish_background(task, result).await;
            active_count.fetch_sub(1, Ordering::Relaxed);
        }
        .instrument(span),
    )
}

fn spawn_background_resume(task: BackgroundTask, prompt: String) -> JoinHandle<()> {
    let span = tracing::Span::current();
    task.active_count.fetch_add(1, Ordering::Relaxed);
    let active_count = Arc::clone(&task.active_count);
    tokio::spawn(
        async move {
            let result = execute_resume(
                &task.db,
                Arc::clone(&task.config),
                &task.agent_name,
                &prompt,
                &task.agent_session_id,
                &task.cancel_token,
                task.cwd.as_ref(),
            )
            .await;

            finish_background(task, result).await;
            active_count.fetch_sub(1, Ordering::Relaxed);
        }
        .instrument(span),
    )
}

async fn finish_background(task: BackgroundTask, result: Result<AgentResult, AgentError>) {
    let (status, transcript, metadata) = match result {
        Ok(agent_result) => {
            *task.metadata_slot.lock().await = Some(agent_result.metadata.clone());
            spawn_children_inner(
                agent_result.spawns,
                &task.db,
                Arc::clone(&task.config),
                &task.agent_session_id,
                task.depth,
            );
            ("ok", agent_result.findings, Some(agent_result.metadata))
        }
        Err(e) => {
            tracing::error!(
                agent_name = task.agent_name.clone(),
                error = e.to_string(),
                "agent failed",
            );
            let partial = last_assistant_message(&task.db, &task.agent_session_id).await;
            ("failed", partial, None)
        }
    };

    // Persist run record
    if let Err(e) = db::agent_runs::finish_run(&task.db, &task.run_id, status, &transcript).await {
        tracing::error!(error = e.to_string(), "failed to finish agent run");
    }

    // Send session event to parent (if there is a parent)
    if let Some(ref parent_id) = task.parent_session_id {
        let system_msg = format!("[agent:{} completed]\n\n{transcript}", task.agent_name);

        // Inject findings as system message in parent session
        if let Err(e) =
            db::sessions::create_message(&task.db, parent_id, "system", &system_msg).await
        {
            tracing::error!(
                error = e.to_string(),
                "failed to inject agent findings into parent session",
            );
        }

        // Send event for continuation
        if let Some(ref tx) = task.event_tx {
            let discord = metadata.map(|m| crate::events::DiscordPayload {
                agent_name: Some(task.agent_name.clone()),
                agent_metadata: Some(m),
                agent_findings: Some(transcript.clone()),
            });
            let _ = tx.send(crate::events::SessionEvent {
                session_id: parent_id.clone(),
                system_message: system_msg,
                discord,
            });
        }
    }

    // Clean up handle from the map
    task.handles.lock().await.remove(&task.agent_session_id);

    tracing::info!(
        agent_name = task.agent_name.clone(),
        status = status,
        "agent finished",
    );
}

/// Spawn child agents from post_completion, enforcing depth limit.
fn spawn_children_inner(
    requests: Vec<SpawnRequest>,
    db: &GhostDb,
    config: Arc<Config>,
    parent_session_id: &str,
    depth: u32,
) {
    let child_depth = depth + 1;
    if child_depth >= MAX_SPAWN_DEPTH {
        if !requests.is_empty() {
            tracing::info!(
                count = requests.len(),
                depth = depth,
                "dropping spawn requests at depth limit",
            );
        }
        return;
    }

    for req in requests {
        let db = db.clone();
        let config = Arc::clone(&config);
        let parent_id = parent_session_id.to_string();
        let span = tracing::Span::current();

        tokio::spawn(
            async move {
                let agent_session_id = match db::sessions::create_agent_session(&db).await {
                    Ok(id) => id,
                    Err(e) => {
                        tracing::error!(
                            agent = req.agent.clone(),
                            error = e.to_string(),
                            "failed to create session for spawned agent",
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
                        tracing::error!(
                            agent = req.agent.clone(),
                            error = e.to_string(),
                            "failed to create run for spawned agent",
                        );
                        return;
                    }
                };

                let cancel_token = CancellationToken::new();

                tracing::info!(
                    agent = req.agent.clone(),
                    parent_session_id = parent_id.clone(),
                    depth = child_depth,
                    "spawning child agent",
                );

                let result = execute_agent(
                    &db,
                    Arc::clone(&config),
                    &req.agent,
                    req.args,
                    &agent_session_id,
                    &cancel_token,
                    Some(&parent_id),
                    None,
                )
                .await;

                let (status, transcript) = match result {
                    Ok(agent_result) => {
                        spawn_children_inner(
                            agent_result.spawns,
                            &db,
                            Arc::clone(&config),
                            &agent_session_id,
                            child_depth,
                        );
                        ("ok", agent_result.findings)
                    }
                    Err(e) => {
                        tracing::error!(
                            agent = req.agent.clone(),
                            error = e.to_string(),
                            "spawned agent failed",
                        );
                        let partial = last_assistant_message(&db, &agent_session_id).await;
                        ("failed", partial)
                    }
                };

                if let Err(e) = db::agent_runs::finish_run(&db, &run_id, status, &transcript).await
                {
                    tracing::error!(error = e.to_string(), "failed to finish spawned agent run");
                }
            }
            .instrument(span),
        );
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
            tracing::error!(
                agent_name = agent_name.to_string(),
                error = e.to_string(),
                "agent execution failed",
            );
            let transcript = format!("Agent error: {e}");
            if let Err(fe) = db::agent_runs::finish_run(db, run_id, "failed", &transcript).await {
                tracing::error!(error = fe.to_string(), "failed to finish agent run");
            }
            return Err(AgentError::ExecutionFailed {
                message: transcript,
            });
        }
    };

    if let Err(e) = db::agent_runs::finish_run(db, run_id, status, &agent_result.findings).await {
        tracing::error!(error = e.to_string(), "failed to finish agent run");
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
/// 1. Agent defaults (threshold=0.90)
/// 2. Lua overrides from `agent_config.compaction` (any field present wins)
/// 3. Global config `instructions` as fallback if the agent didn't specify any
fn build_agent_compaction_config(
    global: &CompactionConfig,
    overrides: Option<&AgentCompactionOverrides>,
) -> CompactionConfig {
    // Agent defaults differ from chat defaults
    let mut cfg = CompactionConfig {
        threshold: 0.90,
        mask_preview_chars: 100,
        instructions: global.instructions.clone(),
        max_tool_result_bytes: global.max_tool_result_bytes,
    };

    if let Some(o) = overrides {
        if let Some(t) = o.threshold {
            cfg.threshold = t;
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
            mask_preview_chars: 100,
            instructions: None,
            max_tool_result_bytes: 30_000,
        }
    }

    #[test]
    fn agent_compaction_defaults_without_overrides() {
        let cfg = build_agent_compaction_config(&global_config(), None);

        assert_eq!(cfg.threshold, 0.90);
        assert_eq!(cfg.mask_preview_chars, 100);
        assert!(cfg.instructions.is_none());
    }

    #[test]
    fn agent_compaction_lua_overrides_win() {
        let overrides = AgentCompactionOverrides {
            threshold: Some(0.70),
            mask_preview_chars: None,
            instructions: Some("Keep all URLs.".into()),
        };

        let cfg = build_agent_compaction_config(&global_config(), Some(&overrides));

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
