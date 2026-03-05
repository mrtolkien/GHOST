# Fix Idle Agent Runaway Loop — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this
> plan task-by-task.

**Goal:** Fix the bug where `chat-reflection` fires every 10 seconds instead of once per
idle period, burning thousands of LLM calls.

**Architecture:** Three layered fixes — (1) make `call_should_trigger` async so Lua
guards actually work, (2) change error policy from fail-open to fail-closed, (3) add
scheduler-level idle dedup so even a broken guard can't cause runaway. Defense in depth.

**Tech Stack:** Rust, mlua (async Lua), tokio

---

## Bug Summary

The `chat-reflection` agent ran **4,444 times in 23 hours** because:

1. `call_should_trigger()` is sync but calls async Lua methods
   (`ctx:list_interface_sessions()`), causing "attempt to yield from outside a
   coroutine" on every invocation
2. The scheduler treats `should_trigger` errors as "proceed anyway" (fail-open)
3. The scheduler has no dedup for idle agents — every 10s tick re-triggers if the guard
   is bypassed

**Evidence:** Logfire span `c154b6a140770f68`, trace `019cbc1ace1523abb377a27ac91ce73d`.
Error logs: `"should_trigger hook error, proceeding anyway"` with
`"attempt to yield from outside a coroutine"` on every single tick from 15:29 UTC Mar 4
onward.

---

### Task 1: Make `call_should_trigger` async

The core bug. `call_should_trigger` uses `f.call()` (sync) but Lua hooks call async ctx
methods. Every other hook (`call_build`, `call_post_completion`, `call_on_resume`) is
already async with `f.call_async().await`.

**Files:**

- Modify: `src/scripting/host.rs:140-163` — change fn signature and call
- Modify: `src/agents/scheduler.rs:210` — add `.await` at cron call site
- Modify: `src/agents/scheduler.rs:301` — add `.await` at idle call site

**Step 1: Write a failing test in `src/scripting/host.rs`**

Add a test that calls an async method inside `should_trigger`. This reproduces the exact
production error. Add it after the existing `should_trigger_returns_false` test (~line
1094):

```rust
#[tokio::test]
async fn should_trigger_with_async_ctx_methods() {
    let dir = test_workspace();
    write_agent_lua(
        dir.path(),
        r#"
        return {
            name = "test",
            description = "test",
            tools = {},
            should_trigger = function(ctx)
                -- This calls an async method; must not error
                local sessions = ctx:list_interface_sessions()
                return #sessions > 0
            end,
        }
        "#,
    );

    let agent_dir = dir.path().join("agents").join("test-agent");
    let mut host = ScriptHost::new(&agent_dir, dir.path()).unwrap();
    host.load_config().unwrap();

    let db = test_db(dir.path()).await;
    let ctx = test_ctx(db, dir.path());
    // No interface sessions in test DB → should return false
    let result = host.call_should_trigger(ctx).await.unwrap();
    assert!(!result);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p ghost should_trigger_with_async -- --nocapture` Expected: Compile
error — `call_should_trigger` is not async, `.await` not valid.

**Step 3: Make `call_should_trigger` async**

In `src/scripting/host.rs`, change `call_should_trigger` (lines 140-163):

```rust
/// Call the `should_trigger(ctx)` hook. Returns whether the agent should run.
pub async fn call_should_trigger(&self, ctx: AgentContext) -> LuaResult<bool> {
    register_ctx(&self.lua, ctx)?;

    let globals = self.lua.globals();
    let agent_table: LuaTable = globals.get("__ghost_agent")?;
    let hook: LuaValue = agent_table.get("should_trigger")?;

    match hook {
        LuaValue::Function(f) => {
            let globals = self.lua.globals();
            let ctx_val: LuaValue = globals.get("ctx")?;
            let result: LuaValue = f.call_async(ctx_val).await?;
            match result {
                LuaValue::Boolean(b) => Ok(b),
                LuaValue::Nil => Ok(true),
                other => Err(LuaError::external(format!(
                    "should_trigger must return boolean or nil, got {other:?}"
                ))),
            }
        }
        LuaValue::Nil => Ok(true),
        _ => Err(LuaError::external("should_trigger must be a function")),
    }
}
```

Two changes: `pub fn` → `pub async fn`, `f.call(ctx_val)?` →
`f.call_async(ctx_val).await?`.

**Step 4: Fix call sites in scheduler**

In `src/agents/scheduler.rs`, add `.await` to both call sites:

Line 210 (in `tick_scheduled`):

```rust
match host.call_should_trigger(ctx).await {
```

Line 301 (in `tick_idle`):

```rust
match host.call_should_trigger(ctx).await {
```

**Step 5: Fix existing tests**

The two existing `should_trigger` tests need `.await` added:

Line 1066:

```rust
assert!(host.call_should_trigger(ctx).await.unwrap());
```

Line 1093:

```rust
assert!(!host.call_should_trigger(ctx).await.unwrap());
```

**Step 6: Run all tests**

Run: `just ci` Expected: All pass, including the new async test.

**Step 7: Commit**

```
git add src/scripting/host.rs src/agents/scheduler.rs
git commit -m "fix: make call_should_trigger async so Lua guards work"
```

---

### Task 2: Fail-closed on `should_trigger` errors

Both `tick_scheduled` and `tick_idle` currently proceed when `should_trigger` errors.
This is the wrong default — a broken guard should block execution, not bypass it. LLM
calls cost money.

**Files:**

- Modify: `src/agents/scheduler.rs:220-226` — change cron error handling
- Modify: `src/agents/scheduler.rs:309-314` — change idle error handling

**Step 1: Change `tick_scheduled` error handler (lines 220-226)**

Replace the `Err` arm to skip instead of proceed:

```rust
Err(e) => {
    logfire::warn!(
        "should_trigger hook error, skipping agent",
        agent_name = name.clone(),
        error = e.to_string(),
    );
    entry.last_run = Some(now);
    entry.next_run = entry.entry.cron.after(&now).next();
    continue;
}
```

**Step 2: Change `tick_idle` error handler (lines 309-314)**

Replace the `Err` arm to skip (`continue` skips to next agent):

```rust
Err(e) => {
    logfire::warn!(
        "should_trigger hook error, skipping agent",
        agent_name = agent.name.clone(),
        error = e.to_string(),
    );
    continue;
}
```

**Step 3: Run tests**

Run: `just ci` Expected: All pass.

**Step 4: Commit**

```
git add src/agents/scheduler.rs
git commit -m "fix: fail-closed on should_trigger errors instead of proceeding"
```

---

### Task 3: Scheduler-level idle dedup

Belt-and-suspenders. Even if `should_trigger` is broken, the scheduler should never
re-trigger an idle agent for a session unless there's new activity. This is a hard guard
at the scheduler level, independent of any Lua hooks.

**Files:**

- Modify: `src/agents/scheduler.rs` — add `last_triggered` tracking to `IdleAgent`,
  update `tick_idle`

**Step 1: Add `last_triggered` field to `IdleAgent`**

Add `HashMap` import at the top of the file. Then modify the `IdleAgent` struct:

```rust
use std::collections::HashMap;

// ...

struct IdleAgent {
    name: String,
    idle_minutes: u64,
    has_should_trigger: bool,
    /// Tracks when we last triggered this agent per session.
    /// Only re-triggers if session has new activity after this timestamp.
    last_triggered: HashMap<String, DateTime<Utc>>,
}
```

**Step 2: Initialize `last_triggered` in `build_entries`**

In the `CrontabTrigger::Idle` arm (around line 169):

```rust
CrontabTrigger::Idle { minutes } => {
    idle_agents.push(IdleAgent {
        name: entry.run.clone(),
        idle_minutes: minutes,
        has_should_trigger,
        last_triggered: HashMap::new(),
    });
}
```

**Step 3: Change `tick_idle` signature to take `&mut [IdleAgent]`**

```rust
async fn tick_idle(
    agent_runner: &AgentRunner,
    db: &GhostDb,
    workspace: &Path,
    idle_agents: &mut [IdleAgent],
) {
```

Also update the call site in `spawn_scheduler` (line 90):

```rust
tick_idle(&agent_runner, &db, &workspace, &mut idle_agents).await;
```

**Step 4: Add the dedup guard in the session loop**

In `tick_idle`, inside the `for record in &sessions` loop, after the `idle_threshold`
check (after line 345), add:

```rust
// Skip if no new activity since we last triggered for this session
if let Some(&triggered_at) = agent.last_triggered.get(&record.session_id) {
    if last_activity <= triggered_at {
        continue;
    }
}

// Mark as triggered BEFORE running — prevents re-trigger on next tick
agent.last_triggered.insert(record.session_id.clone(), now);
```

Place this right before the
`logfire::info!("idle threshold reached, triggering agent", ...)` line.

**Step 5: Run tests**

Run: `just ci` Expected: All pass.

**Step 6: Commit**

```
git add src/agents/scheduler.rs
git commit -m "fix: add scheduler-level idle dedup to prevent runaway triggers"
```

---

### Task 4: Add test for async `should_trigger` with `ctx:get`/`ctx:set`

Verify the agent state methods (used by `chat-reflection`) also work in `should_trigger`
now.

**Files:**

- Modify: `src/scripting/host.rs` — add test after Task 1's test

**Step 1: Write the test**

```rust
#[tokio::test]
async fn should_trigger_with_async_state_methods() {
    let dir = test_workspace();
    write_agent_lua(
        dir.path(),
        r#"
        return {
            name = "test",
            description = "test",
            tools = {},
            should_trigger = function(ctx)
                local val = ctx:get("last_run")
                if val then
                    return false
                end
                return true
            end,
        }
        "#,
    );

    let agent_dir = dir.path().join("agents").join("test-agent");
    let mut host = ScriptHost::new(&agent_dir, dir.path()).unwrap();
    host.load_config().unwrap();

    let db = test_db(dir.path()).await;

    // First call: no state set → should trigger
    let ctx = test_ctx(db.clone(), dir.path());
    assert!(host.call_should_trigger(ctx).await.unwrap());

    // Set state, then check again → should NOT trigger
    crate::db::agent_state::set_state(&db, "test-agent", "last_run", "2026-01-01T00:00:00Z")
        .await
        .unwrap();

    let ctx2 = test_ctx(db, dir.path());
    assert!(!host.call_should_trigger(ctx2).await.unwrap());
}
```

**Step 2: Run tests**

Run: `cargo test -p ghost should_trigger_with_async -- --nocapture` Expected: All three
`should_trigger` tests pass.

**Step 3: Commit**

```
git add src/scripting/host.rs
git commit -m "test: verify async ctx methods work in should_trigger hook"
```

---

## Verification

After all tasks, run the full CI:

```
just ci
```

Then verify the fix addresses the production issue by checking:

1. `call_should_trigger` is async — Lua `ctx:list_interface_sessions()` will no longer
   error
2. Error in `should_trigger` → agent skipped (fail-closed)
3. Idle dedup prevents re-triggering a session with no new activity regardless of
   `should_trigger`
