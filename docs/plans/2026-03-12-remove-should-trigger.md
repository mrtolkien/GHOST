# Remove `should_trigger` — Fix Idle Triggers

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if
> subagents available) or superpowers:executing-plans to implement this plan. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the broken in-memory + Lua `should_trigger` idle trigger system with
fully DB-driven per-session idle detection.

**Architecture:** The scheduler drops all in-memory state (`last_triggered` HashMap,
`has_should_trigger` flags). On each tick, it queries the DB: (1) last message timestamp
per session, (2) whether an agent run already exists after that timestamp. The
`should_trigger` Lua hook is removed entirely.

**Tech Stack:** Rust (sqlx, SQLite), Lua agent files, Astro Starlight docs.

**Spec:** `docs/specs/4-remove-should-trigger.md`

---

## Chunk 1: DB query helpers + scheduler rewrite

### Task 1: Add `last_message_at` DB helper

**Files:**

- Modify: `src/db/sessions.rs`

- [ ] **Step 1: Write the test**

In the `#[cfg(test)]` module of `src/db/sessions.rs`, add:

```rust
#[tokio::test]
async fn last_message_at_returns_none_for_empty_session() {
    let dir = tempfile::TempDir::new().unwrap();
    let db = crate::db::connect(dir.path(), 384).await.unwrap();
    let sid = create_session(&db).await.unwrap();
    assert!(last_message_at(&db, &sid).await.unwrap().is_none());
}

#[tokio::test]
async fn last_message_at_returns_latest() {
    let dir = tempfile::TempDir::new().unwrap();
    let db = crate::db::connect(dir.path(), 384).await.unwrap();
    let sid = create_session(&db).await.unwrap();
    create_message(&db, &sid, "user", "first").await.unwrap();
    create_message(&db, &sid, "assistant", "second").await.unwrap();
    let ts = last_message_at(&db, &sid).await.unwrap().unwrap();
    // Should be the second message's timestamp
    let msgs = list_messages_by_session(&db, &sid).await.unwrap();
    assert_eq!(ts, msgs.last().unwrap().created_at);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib db::sessions::tests::last_message_at -- --nocapture` Expected:
FAIL — `last_message_at` not found.

- [ ] **Step 3: Implement `last_message_at`**

In `src/db/sessions.rs`, add:

```rust
/// Returns the `created_at` of the most recent message in a session, or None.
#[tracing::instrument(skip_all, level = "debug", fields(session_id = %session_id))]
pub async fn last_message_at(
    db: &SqlitePool,
    session_id: &str,
) -> Result<Option<String>, DatabaseError> {
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT created_at FROM message \
         WHERE session_id = ? \
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(session_id)
    .fetch_optional(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "message",
        operation: "last_message_at",
        source,
    })?;
    Ok(row.map(|r| r.0))
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib db::sessions::tests::last_message_at -- --nocapture` Expected:
PASS

- [ ] **Step 5: Commit**

```
git add src/db/sessions.rs
git commit -m "feat: add last_message_at DB query helper"
```

---

### Task 2: Add `has_run_since` DB helper

**Files:**

- Modify: `src/db/agent_runs.rs`

- [ ] **Step 1: Write the test**

In the `#[cfg(test)]` module of `src/db/agent_runs.rs` (create one if it doesn't exist):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn has_run_since_false_when_no_runs() {
        let dir = tempfile::TempDir::new().unwrap();
        let db = crate::db::connect(dir.path(), 384).await.unwrap();
        assert!(!has_run_since(&db, "my-agent", "session-1", "2026-01-01T00:00:00Z")
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn has_run_since_true_when_run_exists_after() {
        let dir = tempfile::TempDir::new().unwrap();
        let db = crate::db::connect(dir.path(), 384).await.unwrap();
        let sid = crate::db::sessions::create_session(&db).await.unwrap();
        let _run_id = create_agent_run(&db, "my-agent", Some(&sid), "agent-sess-1")
            .await
            .unwrap();
        // The run was just created (now), so it's after epoch
        assert!(has_run_since(&db, "my-agent", &sid, "2026-01-01T00:00:00Z")
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn has_run_since_ignores_runs_with_null_session() {
        let dir = tempfile::TempDir::new().unwrap();
        let db = crate::db::connect(dir.path(), 384).await.unwrap();
        // Run with no parent session (e.g. cron-triggered)
        let _run_id = create_agent_run(&db, "my-agent", None, "agent-sess-1")
            .await
            .unwrap();
        assert!(!has_run_since(&db, "my-agent", "some-session", "2026-01-01T00:00:00Z")
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn has_run_since_false_when_run_before_threshold() {
        let dir = tempfile::TempDir::new().unwrap();
        let db = crate::db::connect(dir.path(), 384).await.unwrap();
        let sid = crate::db::sessions::create_session(&db).await.unwrap();
        let _run_id = create_agent_run(&db, "my-agent", Some(&sid), "agent-sess-1")
            .await
            .unwrap();
        // Use a future timestamp — run is before this
        assert!(!has_run_since(&db, "my-agent", &sid, "2099-01-01T00:00:00Z")
            .await
            .unwrap());
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib db::agent_runs::tests::has_run_since -- --nocapture` Expected:
FAIL — `has_run_since` not found.

- [ ] **Step 3: Implement `has_run_since`**

In `src/db/agent_runs.rs`, add:

```rust
/// Check if an agent run exists for the given agent + parent session
/// that started after the given timestamp.
#[tracing::instrument(skip_all, level = "debug", fields(
    agent_name = agent_name,
    session_id = session_id,
))]
pub async fn has_run_since(
    db: &SqlitePool,
    agent_name: &str,
    session_id: &str,
    since: &str,
) -> Result<bool, DatabaseError> {
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT id FROM agent_run \
         WHERE agent_name = ? AND session_id = ? AND started_at > ? \
         LIMIT 1",
    )
    .bind(agent_name)
    .bind(session_id)
    .bind(since)
    .fetch_optional(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "agent_run",
        operation: "has_run_since",
        source,
    })?;
    Ok(row.is_some())
}
```

The pattern matches other queries in this file (`get_run`, etc.).

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib db::agent_runs::tests::has_run_since -- --nocapture` Expected:
PASS

- [ ] **Step 5: Commit**

```
git add src/db/agent_runs.rs
git commit -m "feat: add has_run_since DB query helper"
```

---

### Task 3: Rewrite `tick_idle` to be DB-driven

**Files:**

- Modify: `src/agents/scheduler.rs`

- [ ] **Step 1: Remove in-memory state from `IdleAgent`**

Replace the `IdleAgent` struct with:

```rust
/// Idle agent entry — Lua agent with trigger=after_idle.
#[derive(Debug)]
struct IdleAgent {
    name: String,
    idle_minutes: u64,
}
```

Remove `has_should_trigger` and `last_triggered` fields. Update `build_entries` to stop
loading agent configs — just build `IdleAgent` from the crontab entry directly:

```rust
CrontabTrigger::Idle { minutes } => {
    idle_agents.push(IdleAgent {
        name: entry.run.clone(),
        idle_minutes: minutes,
    });
}
```

Remove the `load_agent` import call and `has_should_trigger` logic from the cron branch
too. The `ScheduleEntry` struct becomes:

```rust
#[derive(Debug)]
struct ScheduleEntry {
    name: String,
    cron: cron::Schedule,
}
```

- [ ] **Step 2: Rewrite `tick_idle`**

Replace the entire `tick_idle` function body with DB-driven logic:

```rust
async fn tick_idle(
    agent_runner: &AgentRunner,
    db: &GhostDb,
    idle_agents: &[IdleAgent],
) {
    if idle_agents.is_empty() {
        return;
    }

    let sessions = match db::interface_sessions::list_all_interface_sessions(db).await {
        Ok(s) => s,
        Err(e) => {
            logfire::warn!(
                "scheduler: failed to list interface sessions for idle check",
                error = e.to_string(),
            );
            return;
        }
    };

    let now = Utc::now();

    for agent in idle_agents {
        let idle_threshold = chrono::Duration::minutes(agent.idle_minutes as i64);

        for record in &sessions {
            // Only check active sessions
            let session = match db::sessions::get_session(db, &record.session_id).await {
                Ok(s) if s.status == "active" => s,
                _ => continue,
            };

            // Step 1: get last message timestamp
            let last_msg_at = match db::sessions::last_message_at(db, &record.session_id).await {
                Ok(Some(ts)) => ts,
                Ok(None) => continue, // no messages — skip
                Err(e) => {
                    logfire::warn!(
                        "scheduler: failed to get last message time",
                        session_id = record.session_id.clone(),
                        error = e.to_string(),
                    );
                    continue;
                }
            };

            let last_msg_dt = match chrono::DateTime::parse_from_rfc3339(&last_msg_at) {
                Ok(dt) => dt.with_timezone(&Utc),
                Err(_) => continue,
            };

            if now - last_msg_dt < idle_threshold {
                continue; // not idle yet
            }

            // Step 2: check if agent already ran for this idle period
            match db::agent_runs::has_run_since(
                db,
                &agent.name,
                &record.session_id,
                &last_msg_at,
            )
            .await
            {
                Ok(true) => continue, // already handled
                Ok(false) => {}
                Err(e) => {
                    logfire::warn!(
                        "scheduler: failed to check agent run history",
                        agent_name = agent.name.clone(),
                        session_id = record.session_id.clone(),
                        error = e.to_string(),
                    );
                    continue;
                }
            }

            logfire::info!(
                "idle threshold reached, triggering agent",
                agent_name = agent.name.clone(),
                session_id = record.session_id.clone(),
                idle_minutes = agent.idle_minutes,
            );

            match agent_runner
                .run(
                    &agent.name,
                    "Execute after idle period.",
                    Some(&record.session_id),
                )
                .await
            {
                Ok(mut result) => {
                    agent_runner.spawn_children(&mut result);
                    logfire::info!("idle agent completed", agent_name = agent.name.clone());
                }
                Err(e) => {
                    logfire::error!(
                        "idle agent failed",
                        agent_name = agent.name.clone(),
                        error = e.to_string(),
                    );
                }
            }
        }
    }
}
```

Update both call sites in the main loop (the interval tick at line 95 and the manual
idle trigger at line 99) — `tick_idle` no longer takes `workspace` or `&mut`:

```rust
tick_idle(&agent_runner, &db, &idle_agents).await;
```

- [ ] **Step 3: Remove `should_trigger` from `tick_scheduled`**

Remove the entire `if entry.entry.has_should_trigger { ... }` block (current lines
211–251). Cron agents fire unconditionally on schedule.

Update `ScheduleEntry` usage — remove `has_should_trigger` field from construction.

After removing the `should_trigger` block, the `workspace` parameter of `tick_scheduled`
becomes unused (it was only needed for `load_agent_with_host`). Remove it from the
function signature and update its call site.

- [ ] **Step 4: Remove unused imports**

Remove `use super::loader::{load_agent, load_agent_with_host};` if no longer used (check
— `load_agent_with_host` was only used for `should_trigger` calls). Remove
`use crate::scripting::AgentContext;` if unused.

- [ ] **Step 5: Run `just ci`**

Run: `just ci` Expected: PASS — format, check, clippy, tests all green.

- [ ] **Step 6: Commit**

```
git add src/agents/scheduler.rs
git commit -m "refactor: rewrite idle triggers to be fully DB-driven

Remove in-memory last_triggered HashMap and should_trigger calls.
Idle detection now queries last message time + agent_run history."
```

---

### Task 4: Change default `scheduler_tick_seconds` to 60

**Files:**

- Modify: `src/config.rs`

- [ ] **Step 1: Change the two defaults from 10 to 60**

In `src/config.rs`, find `.unwrap_or(10)` for `scheduler_tick_seconds` (around line
401–405) and change to `.unwrap_or(60)`. Also change the test default (around line 636)
from `10` to `60`.

- [ ] **Step 2: Run `just ci`**

Run: `just ci` Expected: PASS

- [ ] **Step 3: Commit**

```
git add src/config.rs
git commit -m "chore: change scheduler tick default from 10s to 60s"
```

---

## Chunk 2: Remove `should_trigger` from scripting layer + agent + docs

### Task 5: Remove `call_should_trigger` from ScriptHost

**Files:**

- Modify: `src/scripting/host.rs`
- Modify: `src/scripting/types.rs`

- [ ] **Step 1: Remove `has_should_trigger` from `AgentConfig`**

In `src/scripting/types.rs`, delete the `has_should_trigger: bool` field and its `false`
default.

- [ ] **Step 2: Remove `has_should_trigger` detection from `load_config`**

In `src/scripting/host.rs`, delete the block:

```rust
let has_should_trigger = matches!(
    table.get::<LuaValue>("should_trigger")?,
    LuaValue::Function(_)
);
```

And remove `has_should_trigger` from the `AgentConfig` construction.

- [ ] **Step 3: Remove `call_should_trigger` method**

Delete the entire `pub async fn call_should_trigger` method (lines 139–163 in
`host.rs`).

- [ ] **Step 4: Remove the four `should_trigger` tests**

Delete these test functions from the `#[cfg(test)]` module:

- `should_trigger_returns_true_by_default` (lines 1064–1085)
- `should_trigger_returns_false` (lines 1087–1112)
- `should_trigger_with_async_ctx_methods` (lines 1114–1142)
- `should_trigger_with_async_state_methods` (lines 1144–1182)

Also remove the `assert!(!config.has_should_trigger)` line from the
`load_config_all_hooks` test (around line 719).

- [ ] **Step 5: Add deprecation warning for agents that define `should_trigger`**

In `load_config` in `host.rs`, after loading the agent table, add:

```rust
if matches!(
    table.get::<LuaValue>("should_trigger")?,
    LuaValue::Function(_)
) {
    logfire::warn!(
        "agent defines should_trigger which is no longer supported",
        agent_name = name.clone(),
    );
}
```

- [ ] **Step 6: Run `just ci`**

Run: `just ci` Expected: PASS

- [ ] **Step 7: Commit**

```
git add src/scripting/host.rs src/scripting/types.rs
git commit -m "refactor: remove should_trigger from scripting layer

Drop call_should_trigger, has_should_trigger, and related tests.
Log a deprecation warning if an agent still defines the hook."
```

---

### Task 6: Update chat-reflection agent

**Files:**

- Modify: `assets/agents/chat-reflection/agent.lua`

- [ ] **Step 1: Delete the `should_trigger` function**

Remove lines 20–38 (the `--- Guard:` comment through `end,`):

```lua
    --- Guard: skip if nothing new since last reflection.
    should_trigger = function(ctx)
        ...
    end,
```

- [ ] **Step 2: Verify the agent still loads**

Run: `cargo test --lib scripting -- --nocapture` (any test that exercises agent loading
will catch syntax errors in bundled agents).

- [ ] **Step 3: Commit**

```
git add assets/agents/chat-reflection/agent.lua
git commit -m "refactor: remove should_trigger from chat-reflection agent"
```

---

### Task 7: Update type stubs

**Files:**

- Modify: `assets/agents/.types/ghost.lua`

- [ ] **Step 1: Remove `should_trigger` from type annotation**

Delete this line (around line 191):

```lua
---@field should_trigger? fun(ctx: AgentContext): boolean
```

- [ ] **Step 2: Commit**

```
git add assets/agents/.types/ghost.lua
git commit -m "chore: remove should_trigger from Lua type stubs"
```

---

### Task 8: Update docs

**Files:**

- Modify: `docs/src/content/docs/agents/cron.md`
- Modify: `docs/src/content/docs/agents/syntax.md`

- [ ] **Step 1: Update `cron.md`**

Remove the entire "## `should_trigger` Interaction" section (lines 53–69).

Update the idle entries description to clarify the DB-driven behavior:

````markdown
## Idle Entries

The scheduler polls once per minute and triggers the agent when an active interface
session has been idle (no new messages) for the configured duration. Each idle period
triggers at most one run per agent per session — dedup is handled via the `agent_run`
table.

```lua
{ idle_minutes = 30, run = "chat-reflection" }
```
````

````

- [ ] **Step 2: Update `syntax.md`**

Remove `should_trigger` from the contract example (line 33):

```lua
    should_trigger = function(ctx) return true end, -- for scheduled
````

Remove `should_trigger` from the hooks table (line 85):

```
| `should_trigger` | Before scheduled/idle execution | Boolean |
```

- [ ] **Step 3: Build docs to verify**

Run: `just doc` Expected: Build succeeds without errors.

- [ ] **Step 4: Commit**

```
git add docs/src/content/docs/agents/cron.md docs/src/content/docs/agents/syntax.md
git commit -m "docs: remove should_trigger from agent documentation"
```

---

### Task 9: Update skill file

**Files:**

- Modify: `.agents/skills/lua-scripting/SKILL.md`

- [ ] **Step 1: Remove `should_trigger` references**

Find and remove `has_should_trigger` from the hook presence list and any other mentions
of `should_trigger` in the file.

- [ ] **Step 2: Commit**

```
git add .agents/skills/lua-scripting/SKILL.md
git commit -m "chore: remove should_trigger from lua-scripting skill"
```

---

### Task 10: Final verification

- [ ] **Step 1: Run full CI**

Run: `just ci` Expected: All green — format, check, clippy, tests.

- [ ] **Step 2: Grep for any remaining `should_trigger` references**

Run: `rg should_trigger --type rust --type lua --type md` Expected: Only this spec file
and the plan file. No code references remain.
