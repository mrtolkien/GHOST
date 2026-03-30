# Agent Session Notification Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or superpowers:executing-plans
> to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let Lua agents push messages to user-facing sessions so scheduled agent
findings (e.g., morning briefing) actually reach the user via Discord.

**Architecture:** Add `event_tx` to `AgentContext`, expose two new Lua methods
(`ctx:send_to_session` and `ctx:notify_active_sessions`), extend `SessionEvent` with a
`notify_only` flag so the event handler delivers content without triggering a
continuation chat turn.

**Tech Stack:** mlua (Lua bindings), tokio mpsc (event channel), existing
`SessionEvent`/event handler pipeline, existing `db::sessions` and
`db::interface_sessions` modules.

**Root cause:** Scheduler calls `agent_runner.run()` which returns `AgentResult` with
findings, but nothing delivers them. `AgentContext` has no access to the event pipeline.
Background agents work because `finish_background()` emits `SessionEvent` — but
sync-run agents (used by the scheduler) have no equivalent path.

---

## File Map

| File | Action | Responsibility |
|------|--------|----------------|
| `src/events.rs` | Modify | Add `notify_only` field to `SessionEvent` |
| `src/scripting/bindings.rs` | Modify | Add `event_tx` field, two new Lua methods |
| `src/agents/runner.rs` | Modify | Plumb `event_tx` into `AgentContext` in `setup_agent` and `run_post_completion` |
| `src/daemon/event_handler.rs` | Modify | Skip continuation when `notify_only` is true |
| `src/scripting/bindings.rs` (tests) | Modify | Unit test for `notify_active_sessions` |

---

### Task 1: Add `notify_only` to `SessionEvent`

**Files:**
- Modify: `src/events.rs:7-15`

- [ ] **Step 1: Add the field**

In `src/events.rs`, add `notify_only: bool` to `SessionEvent`:

```rust
pub struct SessionEvent {
    /// Target session ID
    pub session_id: String,
    /// System message to inject before triggering continuation
    pub system_message: String,
    /// Optional metadata for Discord presentation
    pub discord: Option<DiscordPayload>,
    /// When true, deliver content to Discord but skip the continuation chat turn.
    /// Used by agent notifications that don't need GHOST to respond.
    pub notify_only: bool,
}
```

- [ ] **Step 2: Fix all existing `SessionEvent` construction sites**

There are two places that construct `SessionEvent` — both in `src/agents/runner.rs`
(`finish_background`) and potentially in other modules. Add `notify_only: false` to each
existing construction to preserve current behavior:

```bash
rg "SessionEvent {" src/ --files-with-matches
```

For each hit, add `notify_only: false` to the struct literal.

- [ ] **Step 3: Run `just ci` to confirm compilation**

Run: `just ci`
Expected: All green (the new field is set everywhere).

- [ ] **Step 4: Commit**

```
feat(events): add notify_only flag to SessionEvent
```

---

### Task 2: Event handler respects `notify_only`

**Files:**
- Modify: `src/daemon/event_handler.rs:47-157`

- [ ] **Step 1: Modify `handle_event` to skip continuation when `notify_only`**

In `handle_event`, after the Discord agent summary embed send (line ~82), add an early
return when `notify_only` is true. The key change: when `notify_only`, send the
`system_message` content directly to Discord as an assistant-style message and return
without triggering a continuation chat turn.

```rust
async fn handle_event(
    event: SessionEvent,
    session_chat: &SessionChat,
    discord_sender: Option<&DiscordSender>,
    db: &GhostDb,
) {
    let session_id = &event.session_id;

    tracing::info!(
        session_id = session_id.clone(),
        notify_only = event.notify_only,
        "handling session event",
    );

    // Wait for the session to be idle before triggering continuation.
    if !wait_for_idle(db, session_id).await {
        tracing::warn!(
            session_id = session_id.clone(),
            "session not idle after max polls, triggering anyway",
        );
    }

    // Resolve Discord channel
    let discord_channel_id = resolve_discord_channel(db, session_id).await;

    // Send optional agent summary embed to Discord.
    if let Some(ref discord) = event.discord
        && let Some(ref agent_name) = discord.agent_name
        && let Some(ref metadata) = discord.agent_metadata
        && let Some(sender) = discord_sender
        && let Some(channel_id) = discord_channel_id
    {
        let summary = crate::interfaces::discord::ui_events::format_agent_summary(
            agent_name,
            metadata,
            discord.agent_findings.as_deref(),
        );
        let _ = Box::pin(sender.send_compact_container(channel_id, &summary, None)).await;
    }

    // Notify-only: send content to Discord, no continuation turn.
    if event.notify_only {
        if let Some(sender) = discord_sender
            && let Some(channel_id) = discord_channel_id
        {
            if let Err(e) =
                Box::pin(sender.send_to_channel(channel_id, &event.system_message)).await
            {
                tracing::error!(
                    error = e.to_string(),
                    "failed to send notification to Discord",
                );
            }
        }
        return;
    }

    // ... rest of existing handle_event (continuation chat turn) unchanged ...
```

- [ ] **Step 2: Run `just ci`**

Run: `just ci`
Expected: All green.

- [ ] **Step 3: Commit**

```
feat(events): event handler delivers notify_only events without continuation
```

---

### Task 3: Add `event_tx` to `AgentContext` and expose Lua methods

**Files:**
- Modify: `src/scripting/bindings.rs`

- [ ] **Step 1: Add `event_tx` field to `AgentContext`**

```rust
use crate::events::SessionEventSender;

pub struct AgentContext {
    pub db: GhostDb,
    pub workspace: PathBuf,
    pub agent_slug: String,
    pub session_id: String,
    pub trigger_session_id: Option<String>,
    pub spawn_requests: Arc<Mutex<Vec<SpawnRequest>>>,
    pub system_prompt: Arc<Mutex<Option<String>>>,
    pub resume_messages: Arc<Mutex<Option<Vec<LuaMessage>>>>,
    pub config: Option<Arc<Config>>,
    pub tool_manager: Option<Arc<ToolManager>>,
    /// Event sender for session notifications (ctx:send_to_session, ctx:notify_active_sessions).
    pub event_tx: Option<SessionEventSender>,
}
```

Initialize as `None` in `AgentContext::new()`.

- [ ] **Step 2: Add `ctx:send_to_session(session_id, content)` method**

In the `add_methods` block, add:

```rust
// ctx:send_to_session(session_id, content)
methods.add_async_method(
    "send_to_session",
    |_, this, (session_id, content): (String, String)| async move {
        let event_tx = this.event_tx.as_ref().ok_or_else(|| {
            LuaError::external("send_to_session not available: no event channel")
        })?;

        // Inject as system message in the target session
        crate::db::sessions::create_message(&this.db, &session_id, "system", &content)
            .await
            .map_err(|e| LuaError::external(format!("failed to create message: {e}")))?;

        // Emit notify-only event for Discord delivery
        let _ = event_tx.send(crate::events::SessionEvent {
            session_id,
            system_message: content,
            discord: None,
            notify_only: true,
        });

        Ok(())
    },
);
```

- [ ] **Step 3: Add `ctx:notify_active_sessions(content)` convenience method**

```rust
// ctx:notify_active_sessions(content) — send to all sessions with an active interface
methods.add_async_method(
    "notify_active_sessions",
    |_, this, content: String| async move {
        let event_tx = this.event_tx.as_ref().ok_or_else(|| {
            LuaError::external("notify_active_sessions not available: no event channel")
        })?;

        let sessions =
            crate::db::interface_sessions::list_all_interface_sessions(&this.db)
                .await
                .map_err(|e| LuaError::external(e.to_string()))?;

        if sessions.is_empty() {
            tracing::warn!(
                agent = this.agent_slug.clone(),
                "notify_active_sessions: no active interface sessions",
            );
            return Ok(());
        }

        for record in &sessions {
            if let Err(e) = crate::db::sessions::create_message(
                &this.db,
                &record.session_id,
                "system",
                &content,
            )
            .await
            {
                tracing::warn!(
                    session_id = record.session_id.clone(),
                    error = e.to_string(),
                    "notify: failed to create message",
                );
                continue;
            }

            let _ = event_tx.send(crate::events::SessionEvent {
                session_id: record.session_id.clone(),
                system_message: content.clone(),
                discord: None,
                notify_only: true,
            });
        }

        Ok(())
    },
);
```

- [ ] **Step 4: Run `just ci`**

Run: `just ci`
Expected: All green.

- [ ] **Step 5: Commit**

```
feat(scripting): add ctx:send_to_session and ctx:notify_active_sessions
```

---

### Task 4: Plumb `event_tx` through the agent runner

**Files:**
- Modify: `src/agents/runner.rs`

The `event_tx` needs to reach the `AgentContext` in two places: `setup_agent()` and
`run_post_completion()`.

- [ ] **Step 1: Add `event_tx` parameter to `setup_agent`**

Change the signature:

```rust
async fn setup_agent(
    db: &GhostDb,
    config: Arc<Config>,
    agent_name: &str,
    args: HashMap<String, String>,
    agent_session_id: &str,
    parent_session_id: Option<&str>,
    cwd: Option<&PathBuf>,
    event_tx: Option<crate::events::SessionEventSender>,  // NEW
) -> Result<AgentSetup, AgentError> {
```

After the `ctx.with_tool_support(...)` line, set:

```rust
ctx.event_tx = event_tx.clone();
```

Store `event_tx` in `AgentSetup` so `run_post_completion` can use it:

```rust
struct AgentSetup {
    config: AgentConfig,
    build_result: BuildResult,
    script_host: Arc<ScriptHost>,
    session_chat: SessionChat,
    build_spawn_requests: Arc<std::sync::Mutex<Vec<SpawnRequest>>>,
    event_tx: Option<crate::events::SessionEventSender>,  // NEW
}
```

- [ ] **Step 2: Add `event_tx` parameter to `run_post_completion`**

```rust
async fn run_post_completion(
    agent_config: &AgentConfig,
    script_host: &ScriptHost,
    db: &GhostDb,
    config: &Arc<Config>,
    agent_name: &str,
    agent_session_id: &str,
    parent_session_id: Option<&str>,
    event_tx: Option<crate::events::SessionEventSender>,  // NEW
) -> Vec<SpawnRequest> {
```

Set `ctx.event_tx = event_tx;` on the `AgentContext` created inside.

- [ ] **Step 3: Pass `self.event_tx` from `run_with_args` into `execute_agent`**

`execute_agent` needs `event_tx` so it can pass it to `setup_agent` and
`run_post_completion`. Add it to the function signature and to `AgentInvocation` (or as
a separate parameter). Then propagate from `AgentRunner::run_with_args` where
`self.event_tx` is available.

Also do the same for `execute_resume`.

- [ ] **Step 4: Update all call sites**

Update `setup_agent` and `run_post_completion` call sites in:
- `execute_agent` (line ~687)
- `execute_resume` (line ~762) — pass `None` for event_tx here (resume is interactive)
- `setup_resume` (calls `setup_agent` internally, line ~543) — pass `None`
- Background spawn path (`spawn_background_run` and similar) — pass `self.event_tx.clone()`

- [ ] **Step 5: Run `just ci`**

Run: `just ci`
Expected: All green.

- [ ] **Step 6: Commit**

```
feat(agents): plumb event_tx through agent runner to AgentContext
```

---

### Task 5: Update morning-briefing agent to deliver findings

**Files:**
- Modify: `assets/agents/morning-briefing/` (if bundled) or document the change for the
  deployed server's `~/GHOST/agents/morning-briefing/agent.lua`

The morning-briefing agent's `post_completion` hook already exists but only tracks seen
URLs. Add notification delivery.

- [ ] **Step 1: Add notification to `post_completion`**

At the end of the existing `post_completion` function in the morning-briefing agent, add:

```lua
post_completion = function(ctx)
    -- Extract reported URLs from the LLM's markdown output and persist them.
    local messages = ctx:list_messages(ctx.session_id)
    local last = messages[#messages]
    if not last or last.role ~= "assistant" then return end

    local content = last.content or ""

    -- Deliver briefing to user
    ctx:notify_active_sessions(content)

    -- ... existing seen_urls tracking code unchanged ...
```

- [ ] **Step 2: Deploy to server**

```bash
ssh root@192.168.1.3 "cat > ~/GHOST/agents/morning-briefing/agent.lua" < agent.lua
```

Or if the agent is bundled in `assets/agents/`, update the bundled version (it will be
installed on next workspace bootstrap).

- [ ] **Step 3: Manual test**

Run the agent manually on the server to verify delivery:

```bash
ssh root@192.168.1.3 "ghost agent run morning-briefing"
```

Check that the briefing appears in the Discord channel.

- [ ] **Step 4: Commit**

```
feat(agents): morning-briefing delivers findings via notify_active_sessions
```

---

### Task 6: Harden scheduler reload (bonus fix)

**Files:**
- Modify: `src/agents/scheduler.rs:94-103,130-140`

While we're here, fix the silent-wipe bug from the diagnosis.

- [ ] **Step 1: On reload failure, keep old entries**

In the `tokio::select!` file-change branch and config-change branch, only replace
entries if `build_entries` succeeds:

```rust
// File change branch
path = fs_rx.recv() => {
    if let Some(_path) = path {
        tokio::time::sleep(FILE_CHANGE_DEBOUNCE).await;
        while fs_rx.try_recv().is_ok() {}

        info!("agent files changed, reloading");
        let (new_scheduled, new_idle) = build_entries(&workspace);

        // Only replace if we got entries (or the crontab is intentionally empty)
        if !new_scheduled.is_empty() || !new_idle.is_empty() {
            scheduled = new_scheduled;
            idle_agents = new_idle;
            info!(
                scheduled_count = scheduled.len(),
                idle_count = idle_agents.len(),
                "scheduler entries reloaded",
            );
        } else {
            tracing::warn!("reload returned zero entries, keeping previous entries");
        }
    }
}
```

Same pattern for the `config.changed()` branch.

- [ ] **Step 2: Log counts after reload**

Already included in the code above — the `info!` with `scheduled_count` and
`idle_count` after successful reload.

- [ ] **Step 3: Change `build_entries` error log to ERROR level**

In `build_entries`, line 138:

```rust
Err(e) => {
    tracing::error!(error = e.clone(), "scheduler: failed to load crontab");
    return (scheduled, idle_agents);
}
```

Change from `warn!` to `error!` — a failed crontab load is always a problem.

- [ ] **Step 4: Run `just ci`**

Run: `just ci`
Expected: All green.

- [ ] **Step 5: Commit**

```
fix(scheduler): keep old entries on reload failure, log counts after reload
```
