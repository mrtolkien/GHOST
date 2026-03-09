# Daemon-Level E2E Testing — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Enable true e2e tests that boot the real daemon (minus Discord), send messages, wait for quiescence, and assert on workspace/DB state.

**Architecture:** Make `boot()` return a `DaemonHandle` struct (replacing the tuple). Discord skips gracefully when no token is set. Scheduler gets a manual trigger channel for idle agents. `DaemonHandle::settle()` polls busy counters across all subsystems.

**Tech Stack:** Rust, tokio, AtomicUsize counters, mpsc channels, existing LiveTestEnv harness.

**Design doc:** `docs/plans/2026-03-09-daemon-e2e-testing-design.md`

---

### Task 1: Make `start_discord()` return `Ok(None)` on missing token

Currently `start_discord()` returns `Err(DiscordError::MissingToken)` when `DISCORD_BOT_TOKEN` is unset. This makes `boot()` fail in test environments. Change it to return `Ok(None)` with an info log, like the `!config.discord.enabled` path already does.

**Files:**
- Modify: `src/interfaces/discord/start.rs:101-105`

**Step 1: Change the MissingToken behavior**

In `start_discord()`, replace the token error paths with `Ok(None)`:

```rust
// Before (line 101):
let token = std::env::var("DISCORD_BOT_TOKEN").map_err(|_| DiscordError::MissingToken)?;
if token.is_empty() {
    return Err(DiscordError::MissingToken);
}

// After:
let token = match std::env::var("DISCORD_BOT_TOKEN") {
    Ok(t) if !t.is_empty() => t,
    _ => {
        info!("DISCORD_BOT_TOKEN not set, skipping Discord");
        return Ok(None);
    }
};
```

**Step 2: Run `just ci`**

Expected: PASS — existing behavior unchanged when token IS present.

**Step 3: Commit**

```bash
git add src/interfaces/discord/start.rs
git commit -m "feat: gracefully skip Discord when bot token is missing"
```

---

### Task 2: Replace `BootResult` tuple with `DaemonHandle` struct

The current `boot()` returns a 6-element tuple. Replace with a named struct that exposes what tests (and future CLI) need.

**Files:**
- Modify: `src/daemon/run.rs`

**Step 1: Define `DaemonHandle`**

Replace the `BootResult` type alias with:

```rust
/// Handle to a running GHOST daemon. Returned by `boot()`.
pub struct DaemonHandle {
    pub session_chat: Arc<SessionChat>,
    pub db: GhostDb,
    pub config: Config,
    pub agent_runner: Arc<AgentRunner>,
    pub active_sessions: ActiveSessions,
    shutdown_tx: watch::Sender<bool>,
    handles: Vec<JoinHandle<()>>,
    discord: Option<(DiscordSender, JoinHandle<()>)>,
}

impl DaemonHandle {
    /// Signal all subsystems to shut down and wait for them.
    pub async fn shutdown(self) {
        let _ = self.shutdown_tx.send(true);
        for h in self.handles {
            let _ = h.await;
        }
        if let Some((_, h)) = self.discord {
            let _ = h.await;
        }
    }
}
```

**Step 2: Update `boot()` to return `DaemonHandle`**

Change the return type and construct the struct at the end instead of the tuple. Keep all existing logic identical.

**Step 3: Update `run()` to use `DaemonHandle`**

```rust
pub async fn run() -> Result<(), GhostError> {
    let handle = boot().await?;

    if handle.discord.is_some() {
        info!("GHOST daemon running — press Ctrl+C to stop");
    } else {
        info!("No interfaces enabled. Waiting for Ctrl+C...");
    }

    tokio::signal::ctrl_c().await.ok();
    info!("Ctrl+C received, shutting down...");
    handle.shutdown().await;

    info!("GHOST daemon stopped");
    Ok(())
}
```

Note: `run()` currently waits on the Discord handle. With `DaemonHandle`, `shutdown()` handles all joins. The Ctrl+C select is still the main wait point.

**Step 4: Run `just ci`**

Expected: PASS.

**Step 5: Commit**

```bash
git add src/daemon/run.rs
git commit -m "refactor: replace BootResult tuple with DaemonHandle struct"
```

---

### Task 3: Add manual trigger channel to scheduler

The scheduler currently only fires idle agents on its timer tick. Add an mpsc channel that, when poked, immediately runs `tick_idle()`.

**Files:**
- Modify: `src/agents/scheduler.rs:55-116`
- Modify: `src/daemon/run.rs` (pass trigger_rx to scheduler, store trigger_tx in DaemonHandle)

**Step 1: Add trigger channel to `spawn_scheduler`**

```rust
pub fn spawn_scheduler(
    agent_runner: Arc<AgentRunner>,
    config: Config,
    db: GhostDb,
    mut shutdown: watch::Receiver<bool>,
    mut idle_trigger_rx: mpsc::Receiver<()>,  // NEW
) -> JoinHandle<()> {
```

Add a new arm to the `select!` loop:

```rust
loop {
    tokio::select! {
        _ = interval.tick() => {
            tick_scheduled(&agent_runner, &db, &workspace, &mut scheduled).await;
            tick_idle(&agent_runner, &db, &workspace, &mut idle_agents).await;
        }
        Some(()) = idle_trigger_rx.recv() => {
            info!("manual idle trigger received");
            tick_idle(&agent_runner, &db, &workspace, &mut idle_agents).await;
        }
        // ... existing arms unchanged
    }
}
```

**Step 2: Wire the channel in `boot()` and `DaemonHandle`**

In `src/daemon/run.rs`:

```rust
let (idle_trigger_tx, idle_trigger_rx) = mpsc::channel::<()>(8);

let scheduler_handle = crate::agents::scheduler::spawn_scheduler(
    Arc::clone(&agent_runner),
    config.clone(),
    db.clone(),
    shutdown_rx.clone(),
    idle_trigger_rx,  // NEW
);
```

Add to `DaemonHandle`:

```rust
pub struct DaemonHandle {
    // ... existing fields
    idle_trigger_tx: mpsc::Sender<()>,
}

impl DaemonHandle {
    /// Trigger idle agents immediately (reflection, etc.).
    pub async fn trigger_idle_agents(&self) {
        let _ = self.idle_trigger_tx.send(()).await;
    }
}
```

**Step 3: Run `just ci`**

Expected: PASS.

**Step 4: Commit**

```bash
git add src/agents/scheduler.rs src/daemon/run.rs
git commit -m "feat: add manual trigger channel for idle agents"
```

---

### Task 4: Add busy counters to subsystems

`settle()` needs to know when each subsystem is idle. Add `Arc<AtomicUsize>` counters to the subsystems that do background work.

**Files:**
- Modify: `src/agents/runner.rs` — add `active_count: Arc<AtomicUsize>`
- Modify: `src/tools/shell.rs` — add `BACKGROUND_SHELL_COUNT: AtomicUsize` (module-level static) or pass via `ToolContext`
- Modify: `src/daemon/watcher.rs` — add `watcher_busy: Arc<AtomicBool>` param
- Modify: `src/daemon/run.rs` — wire counters into `DaemonHandle`

**Step 1: AgentRunner — active task counter**

In `src/agents/runner.rs`, add to `AgentRunner`:

```rust
pub struct AgentRunner {
    // ... existing fields
    active_count: Arc<AtomicUsize>,
}
```

Increment in `run_in_background()` before spawning, decrement in the spawned task's completion (both success and error paths). Expose:

```rust
impl AgentRunner {
    pub fn active_count(&self) -> usize {
        self.active_count.load(Ordering::Relaxed)
    }
}
```

**Step 2: Background shell counter**

In `src/tools/shell.rs`, add a static or pass an `Arc<AtomicUsize>` through `ToolContext`. The static approach is simpler for now:

```rust
static BACKGROUND_SHELL_COUNT: AtomicUsize = AtomicUsize::new(0);

pub fn background_shell_count() -> usize {
    BACKGROUND_SHELL_COUNT.load(Ordering::Relaxed)
}
```

Increment before `tokio::spawn`, decrement at the end of the spawned future.

**Step 3: File watcher busy flag**

In `src/daemon/watcher.rs`, accept an `Arc<AtomicBool>` param in `spawn_watcher()`. Set to `true` before `process_batch()`, set to `false` after. Expose via `DaemonHandle`.

**Step 4: Wire into DaemonHandle**

```rust
pub struct DaemonHandle {
    // ... existing fields
    watcher_busy: Arc<AtomicBool>,
}

impl DaemonHandle {
    pub fn is_idle(&self) -> bool {
        self.active_sessions.is_empty()
            && self.agent_runner.active_count() == 0
            && !self.watcher_busy.load(Ordering::Relaxed)
            && crate::tools::shell::background_shell_count() == 0
    }
}
```

**Step 5: Run `just ci`**

Expected: PASS.

**Step 6: Commit**

```bash
git add src/agents/runner.rs src/tools/shell.rs src/daemon/watcher.rs src/daemon/run.rs
git commit -m "feat: add busy counters to subsystems for settle() support"
```

---

### Task 5: Implement `settle()` on DaemonHandle

**Files:**
- Modify: `src/daemon/run.rs`

**Step 1: Implement settle**

```rust
use std::time::{Duration, Instant};

#[derive(Debug, thiserror::Error)]
#[error("system did not settle within {0:?}")]
pub struct SettleTimeout(Duration);

impl DaemonHandle {
    /// Wait until all subsystems are idle, or timeout.
    pub async fn settle(&self) -> Result<(), SettleTimeout> {
        self.settle_with_timeout(Duration::from_secs(180)).await
    }

    pub async fn settle_with_timeout(&self, timeout: Duration) -> Result<(), SettleTimeout> {
        let deadline = Instant::now() + timeout;
        let poll = Duration::from_millis(500);

        loop {
            if self.is_idle() {
                // Stay idle for one more poll to catch races
                tokio::time::sleep(poll).await;
                if self.is_idle() {
                    return Ok(());
                }
            }

            if Instant::now() >= deadline {
                return Err(SettleTimeout(timeout));
            }

            tokio::time::sleep(poll).await;
        }
    }
}
```

**Step 2: Run `just ci`**

Expected: PASS.

**Step 3: Commit**

```bash
git add src/daemon/run.rs
git commit -m "feat: implement DaemonHandle::settle() for quiescence waiting"
```

---

### Task 6: Add `boot()` to LiveTestEnv

Wire `LiveTestEnv` to call the real `boot()` with a test config (no Discord token, temp workspace).

**Files:**
- Modify: `tests/common.rs`

**Step 1: Add boot method**

```rust
#[cfg(feature = "live-tests")]
impl LiveTestEnv {
    /// Boot the real daemon with this test's config. Discord is skipped
    /// because no DISCORD_BOT_TOKEN is set in the test environment.
    pub async fn boot(&self) -> ghost::daemon::DaemonHandle {
        // Temporarily override config loading to use our test config.
        // boot() calls config::load() internally, so we need to set
        // GHOST_CONFIG_DIR to point at our test config dir.
        std::env::set_var("GHOST_CONFIG_DIR", self.config_dir_path());
        // Ensure no Discord token leaks from the host env
        std::env::remove_var("DISCORD_BOT_TOKEN");

        ghost::daemon::boot().await.expect("daemon boot failed")
    }
}
```

Note: `boot()` loads config internally via `config::load()`. We override `GHOST_CONFIG_DIR` so it picks up the test config. This is the same mechanism the production code uses. If `boot()` doesn't support this cleanly, we may need to add a `boot_with_config(config)` variant — but check first.

Actually, looking at `boot()`, it calls `crate::config::load()` which reads from the default config dir. For tests, we need either:
- (a) A `boot_with_config(config: Config)` variant, or
- (b) Set `GHOST_CONFIG_DIR` env var before calling `boot()`

Option (a) is cleaner — add `pub async fn boot_with_config(config: Config) -> Result<DaemonHandle, GhostError>` and have `boot()` call it after loading config. Then `LiveTestEnv::boot()` passes its pre-built test config directly.

**Step 2: Run `just ci`**

Expected: PASS (no tests call `boot()` yet).

**Step 3: Commit**

```bash
git add tests/common.rs src/daemon/run.rs
git commit -m "feat: add boot_with_config() and LiveTestEnv::boot()"
```

---

### Task 7: Write the Ark Nova e2e test

The first real daemon-level test. Exercises: chat → tool use → document import → file watcher → chunking → embedding → semantic search.

**Files:**
- Create: `tests/daemon_e2e.rs`

**Step 1: Write the test**

```rust
//! Daemon-level e2e tests — boot the real daemon, send messages, assert on state.
#![cfg(feature = "live-tests")]

mod common;

use common::LiveTestEnv;

/// Test: import a PDF reference, verify it gets chunked, embedded, and is
/// searchable with relevant snippets.
#[tokio::test]
async fn test_ark_nova_import() {
    let env = LiveTestEnv::new("ark_nova_import").await;
    let daemon = env.boot().await;

    // ACT: ask GHOST to import the Ark Nova rules
    let session_id = ghost::db::sessions::create_session(&daemon.db)
        .await
        .expect("create session");

    daemon
        .session_chat
        .chat(&session_id, "Import the Ark Nova rules for future reference", None, None)
        .await
        .expect("chat failed");

    daemon.settle().await.expect("settle after chat");

    // Trigger reflection (idle agents) and let everything finish
    daemon.trigger_idle_agents().await;
    daemon.settle().await.expect("settle after reflection");

    // ASSERT 1: A PDF was imported and converted to .md
    let refs_dir = daemon.config.workspace.join("references");
    let md_files: Vec<_> = walkdir::WalkDir::new(&refs_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "md"))
        .collect();
    assert!(
        !md_files.is_empty(),
        "expected at least one .md reference file under {}, found none",
        refs_dir.display()
    );

    // ASSERT 2: 50+ chunks with associated embedding vectors
    let all_embeddings = ghost::db::embeddings::list_all_embeddings(&daemon.db)
        .await
        .expect("list embeddings");

    // Filter to reference embeddings (source_type = "reference")
    let ref_embeddings: Vec<_> = all_embeddings
        .iter()
        .filter(|e| e.source_type == "reference")
        .collect();

    assert!(
        ref_embeddings.len() >= 50,
        "expected 50+ reference embedding chunks, got {}",
        ref_embeddings.len()
    );

    // Verify all chunks have vectors (not null)
    let with_vectors = ref_embeddings.iter().filter(|e| e.has_vector).count();
    assert_eq!(
        ref_embeddings.len(),
        with_vectors,
        "all chunks should have embedding vectors"
    );

    // ASSERT 3: Semantic search for "break rules" returns relevant snippets
    let results = ghost::db::knowledge::search_knowledge(
        &daemon.db,
        "ark nova break rules",
        10,
    )
    .await
    .expect("knowledge search");

    let all_snippets: String = results
        .iter()
        .map(|r| r.snippet.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        all_snippets.to_lowercase().contains("break"),
        "search for 'ark nova break rules' should return snippets mentioning breaks.\n\
         Got snippets:\n{all_snippets}"
    );

    // Log session for diagnostics
    env.log_session_json(&session_id, "ark_nova_chat").await;
}
```

Note: The exact function signatures for `search_knowledge`, `list_all_embeddings`, etc. need to be verified against the actual DB module API. The test above is the intent — adjust field names and function signatures to match the real code during implementation.

**Step 2: Run the test**

```bash
cargo test --features live-tests test_ark_nova_import -- --nocapture
```

Expected: The test should run but **ASSERT 3 is expected to fail** — this validates that we're catching the real snippet retrieval issue.

**Step 3: Commit**

```bash
git add tests/daemon_e2e.rs
git commit -m "test: add first daemon-level e2e test (ark nova import)"
```

---

### Summary of changes

| Task | Files | Purpose |
|------|-------|---------|
| 1 | `discord/start.rs` | Graceful skip on missing token |
| 2 | `daemon/run.rs` | `DaemonHandle` struct |
| 3 | `scheduler.rs`, `daemon/run.rs` | Manual idle trigger |
| 4 | `runner.rs`, `shell.rs`, `watcher.rs`, `run.rs` | Busy counters |
| 5 | `daemon/run.rs` | `settle()` implementation |
| 6 | `common.rs`, `daemon/run.rs` | `boot_with_config()` + test harness |
| 7 | `daemon_e2e.rs` | Ark Nova import test |

Tasks 1–3 can be done in parallel. Task 4 depends on nothing. Task 5 depends on 4. Task 6 depends on 2+5. Task 7 depends on 6.

### IMPORTANT: Live test configuration

These tests run against real services (LLM providers, Ollama for embeddings, docling for PDF conversion). If any configuration, environment variable, or service connectivity issue arises during implementation or test runs, **STOP and ask the user**. Do NOT:
- Guess at config values or API keys
- Skip failing subsystems to make the test pass
- Stub out real services to work around config issues
- Weaken assertions because a service didn't respond as expected

The entire point of these tests is to exercise the real system. If something isn't configured right, the user needs to know.
