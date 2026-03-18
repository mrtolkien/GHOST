# Config Reload Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or superpowers:executing-plans
> to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Hot-reload `config.toml` and `.env` at runtime via `ghost config reload`
without restarting the daemon or breaking active chat sessions.

**Architecture:** A `tokio::sync::watch` channel distributes `Arc<Config>` snapshots to
all consumers. The CLI validates the new config and sends SIGHUP via systemd/launchd.
The daemon catches SIGHUP, re-reads `.env` + `config.toml`, validates immutable fields,
and publishes the new config through the watch channel.

**Tech Stack:** tokio::sync::watch, std::sync::Arc, Unix signals (SIGHUP),
systemd/launchd

**Spec:** `backlog/tasks/4-easy-install/1-config-reload.md`

---

## File Map

### New files

| File                | Purpose                              |
| ------------------- | ------------------------------------ |
| `src/cli/reload.rs` | `ghost config reload` CLI subcommand |

### Modified files

| File                                 | What changes                                                                                              |
| ------------------------------------ | --------------------------------------------------------------------------------------------------------- |
| `src/config.rs`                      | Add `SharedConfig`, `ConfigSender`, `SharedConfigExt` trait, `reload()` fn, `ImmutableFieldChanged` error |
| `src/daemon/run.rs`                  | Watch channel creation, SIGHUP handler loop, distribute `SharedConfig`                                    |
| `src/chat/session.rs`                | `config: Config` → `config: SharedConfig`, accessor returns `Arc<Config>`                                 |
| `src/chat/compaction.rs`             | Use `Arc<Config>` from accessor (follows session.rs change)                                               |
| `src/chat/tool_loop.rs`              | Use `Arc<Config>` from accessor                                                                           |
| `src/agents/runner.rs`               | `config: Config` → `config: SharedConfig`, snapshot at spawn time                                         |
| `src/agents/scheduler.rs`            | `config: Config` → `config: SharedConfig`, add `.changed()` branch                                        |
| `src/daemon/watcher.rs`              | `workspace + EmbeddingsConfig` → `SharedConfig`                                                           |
| `src/daemon/event_handler.rs`        | Use `Arc<Config>` from session_chat accessor                                                              |
| `src/prompt/renderer.rs`             | `config: Config` → `config: SharedConfig`                                                                 |
| `src/tools/context.rs`               | `config: Config` → `config: Arc<Config>`                                                                  |
| `src/interfaces/discord/bot.rs`      | `config: Config` → `config: SharedConfig` in Handler                                                      |
| `src/interfaces/discord/start.rs`    | Pass `SharedConfig` to Handler                                                                            |
| `src/scripting/bindings.rs`          | `config: Option<Config>` → `config: Option<Arc<Config>>`                                                  |
| `src/cli/config.rs`                  | Add `Reload` variant                                                                                      |
| `src/cli/mod.rs`                     | Add `pub mod reload;`                                                                                     |
| `src/cli/browsers.rs`                | "ghost reboot" → "ghost config reload"                                                                    |
| `assets/skills/browser-use/skill.md` | "ghost reboot" → "ghost config reload"                                                                    |

---

## Task 1: SharedConfig type + reload function

**Files:**

- Modify: `src/config.rs`

This is the foundation. Everything else builds on these types.

- [ ] **Step 1: Add the `ImmutableFieldChanged` error variant**

In the `ConfigError` enum, add:

```rust
#[error("cannot change '{field}' at runtime (requires restart)")]
ImmutableFieldChanged { field: String },
```

- [ ] **Step 2: Add SharedConfig types and trait**

At the top of `src/config.rs`, add the necessary imports (`Arc`, `watch`) and then after
the `Config` struct and its impl block, add:

```rust
/// A shared, dynamically-reloadable config handle.
/// Cheap to clone — distribute to all consumers.
pub type SharedConfig = tokio::sync::watch::Receiver<Arc<Config>>;

/// Sender half — held by the daemon to publish config updates.
pub(crate) type ConfigSender = tokio::sync::watch::Sender<Arc<Config>>;

/// Convenience: grab a snapshot without dealing with borrow lifetimes.
pub trait SharedConfigExt {
    fn current(&self) -> Arc<Config>;
}

impl SharedConfigExt for SharedConfig {
    fn current(&self) -> Arc<Config> {
        self.borrow().clone()
    }
}
```

- [ ] **Step 3: Add `reload()` function**

This bypasses the `Once` guard so `.env` changes are picked up:

```rust
/// Re-read `.env` and `config.toml` for hot-reload.
///
/// Unlike `load()`, this always re-reads `.env` (bypassing the `Once` guard)
/// so that environment variable changes take effect.
pub fn reload() -> Result<Config, ConfigError> {
    // Re-read .env (bypass Once guard)
    load_dotenv_from_config_dir();
    load_from_dir(&config_dir()?)
}
```

- [ ] **Step 4: Add `validate_reload()` function**

```rust
/// Check that immutable fields haven't changed between old and new config.
pub fn validate_reload(current: &Config, new: &Config) -> Result<(), ConfigError> {
    if current.workspace != new.workspace {
        return Err(ConfigError::ImmutableFieldChanged {
            field: "workspace".into(),
        });
    }
    if current.embeddings.dimension != new.embeddings.dimension {
        return Err(ConfigError::ImmutableFieldChanged {
            field: "embeddings.dimension".into(),
        });
    }
    if current.discord.enabled != new.discord.enabled {
        return Err(ConfigError::ImmutableFieldChanged {
            field: "discord.enabled".into(),
        });
    }
    Ok(())
}
```

- [ ] **Step 5: Write unit tests**

```rust
#[cfg(test)]
mod reload_tests {
    use super::*;

    #[test]
    fn validate_reload_accepts_identical_config() {
        let workspace = std::path::Path::new("/tmp/test");
        let config = test_config(workspace);
        assert!(validate_reload(&config, &config).is_ok());
    }

    #[test]
    fn validate_reload_rejects_workspace_change() {
        let a = test_config(std::path::Path::new("/tmp/a"));
        let b = test_config(std::path::Path::new("/tmp/b"));
        let err = validate_reload(&a, &b).unwrap_err();
        assert!(matches!(err, ConfigError::ImmutableFieldChanged { .. }));
    }

    #[test]
    fn validate_reload_rejects_dimension_change() {
        let workspace = std::path::Path::new("/tmp/test");
        let mut a = test_config(workspace);
        let mut b = test_config(workspace);
        a.embeddings.dimension = 1024;
        b.embeddings.dimension = 768;
        let err = validate_reload(&a, &b).unwrap_err();
        assert!(matches!(err, ConfigError::ImmutableFieldChanged { .. }));
    }

    #[test]
    fn shared_config_current_returns_snapshot() {
        let config = test_config(std::path::Path::new("/tmp/test"));
        let (tx, rx) = tokio::sync::watch::channel(Arc::new(config.clone()));
        let snapshot = rx.current();
        assert_eq!(snapshot.workspace, config.workspace);

        // Update and verify new snapshot
        let mut updated = config;
        updated.debug.save_requests = true;
        tx.send(Arc::new(updated)).unwrap();
        let new_snapshot = rx.current();
        assert!(new_snapshot.debug.save_requests);
    }
}
```

- [ ] **Step 6: Run tests and verify**

Run: `cargo test --lib config::reload_tests` Expected: all 4 tests pass.

- [ ] **Step 7: Commit**

```
feat: add SharedConfig type, reload function, and immutable field validation
```

---

## Task 2: Migrate ToolContext to `Arc<Config>`

**Files:**

- Modify: `src/tools/context.rs`

The simplest consumer — change the field type from `Config` to `Arc<Config>`. This is a
prerequisite for the SessionChat migration since SessionChat creates ToolContext.

- [ ] **Step 1: Change the field type**

In `ToolContext`, change:

```rust
pub config: Config,
```

to:

```rust
pub config: Arc<Config>,
```

Add `use std::sync::Arc;` at the top.

- [ ] **Step 2: Fix all compilation errors from this change**

Grep for `tool_ctx.config` and `ctx.config` across the tools directory. Most accesses go
through `&self.config.field` which works identically on `Arc<Config>`. The main change
is anywhere that clones or moves the config — it now clones the `Arc`, not the `Config`.

Key files that create a `ToolContext`:

- `src/chat/session.rs:549-560` — builds `ToolContext` with
  `config: self.config.clone()`
- `src/scripting/bindings.rs` — when `ctx:call_tool()` creates a ToolContext

For now, just get these compiling. The session.rs call site will still clone
`self.config` (which is still `Config` at this point) — wrap it in
`Arc::new(self.config.clone())`. This is temporary; Task 3 will change session.rs to use
SharedConfig.

- [ ] **Step 3: Run `just ci` and fix any issues**

- [ ] **Step 4: Commit**

```
refactor: change ToolContext.config to Arc<Config>
```

---

## Task 3: Migrate SessionChat, PromptRenderer, AgentRunner, and AgentContext

**Files:**

- Modify: `src/chat/session.rs`
- Modify: `src/prompt/renderer.rs`
- Modify: `src/chat/compaction.rs`
- Modify: `src/chat/tool_loop.rs`
- Modify: `src/daemon/event_handler.rs`
- Modify: `src/agents/runner.rs`
- Modify: `src/scripting/bindings.rs`

These are merged into one task because `execute_agent` in runner.rs calls
`SessionChat::new()` — changing the signature in session.rs without updating runner.rs
would leave a non-compiling state.

- [ ] **Step 1: Migrate PromptRenderer**

In `src/prompt/renderer.rs`, change `config: Config` to `config: SharedConfig`. Update
`new()` to take `SharedConfig`. In `render_system_prompt()`, call
`self.config.current()` to get the workspace. Add import:
`use crate::config::{SharedConfig, SharedConfigExt};`

- [ ] **Step 2: Migrate SessionChat fields and constructor**

In `src/chat/session.rs`, change `config: Config` to `config: SharedConfig`.

Update `from_config()` to accept `SharedConfig`:

```rust
pub fn from_config(db: GhostDb, config: SharedConfig) -> Result<Self, ChatError> {
    let cfg = config.current();
    let provider = provider_for_alias(&cfg, None)?;
    // ...
}
```

Similarly update `SessionChat::new()` to take `SharedConfig`.

- [ ] **Step 3: Update the `config()` accessor**

Change `pub fn config(&self) -> &Config` to `pub fn config(&self) -> Arc<Config>` that
calls `self.config.current()`. Callers get an `Arc<Config>` snapshot.

- [ ] **Step 4: Update SessionChat's internal config access**

Methods `default_model_name()`, `model_context_window()`, `model_reasoning_effort()`,
`compaction_config()`, and `execute_single_tool` all do `self.config.field`. Change to:

```rust
let config = self.config.current();
// then use config.field
```

For `execute_single_tool`, the ToolContext creation becomes:

```rust
let config = self.config.current();
let tool_ctx = ToolContext {
    workspace: config.workspace.clone(),
    config,  // Arc<Config>
    // ...
};
```

- [ ] **Step 5: Fix callers of `session_chat.config()`**

- `src/chat/compaction.rs` — bind to local: `let config = self.config();`
- `src/chat/tool_loop.rs:129` — bind to local first
- `src/daemon/event_handler.rs:225` — `Arc<Config>` derefs to `&Config`, should work

- [ ] **Step 6: Migrate AgentRunner**

Change `config: Config` to `config: SharedConfig` in the struct and `new()`.

- [ ] **Step 7: Update AgentRunner's internal config access**

- `self.config.workspace` → `self.config.current().workspace`
- `self.config.clone()` (BackgroundTask, execute_agent) → `self.config.current()`

Change `execute_agent()` to take `config: Arc<Config>` (not `&Config`). This is
important: `execute_agent` calls `ctx.with_tool_support(config.clone(), ...)` and
`SessionChat::new(..., config)`. With `Arc<Config>`:

- `config.clone()` produces `Arc<Config>` (cheap refcount bump)
- `with_tool_support` gets `Arc<Config>` (matches AgentContext field)
- For `SessionChat::new()` inside `execute_agent`: create a one-shot watch channel
  `let (_, cfg_rx) = watch::channel(config.clone());` and pass `cfg_rx`

Similarly update `setup_agent` and `setup_resume` which also call `with_tool_support`.

- [ ] **Step 8: Update BackgroundTask**

Change `BackgroundTask.config: Config` to `config: Arc<Config>`. Snapshot at spawn time
from runner's `.current()`.

- [ ] **Step 9: Migrate AgentContext**

In `src/scripting/bindings.rs`, change `config: Option<Config>` to
`config: Option<Arc<Config>>`. Update `with_tool_support()` to take `Arc<Config>`.

- [ ] **Step 10: Run `just ci` and fix any issues**

- [ ] **Step 11: Commit**

```
refactor: migrate SessionChat, PromptRenderer, AgentRunner, AgentContext to SharedConfig
```

---

## Task 4: Migrate Scheduler

**Files:**

- Modify: `src/agents/scheduler.rs`

The scheduler caches `tick_secs` from config. Add a `.changed()` branch to refresh it.

- [ ] **Step 1: Change function signature**

```rust
pub fn spawn_scheduler(
    agent_runner: Arc<AgentRunner>,
    mut config: SharedConfig,  // was: config: Config
    db: GhostDb,
    mut shutdown: watch::Receiver<bool>,
    mut idle_trigger_rx: mpsc::Receiver<()>,
) -> JoinHandle<()> {
```

Add import: `use crate::config::{SharedConfig, SharedConfigExt};`

- [ ] **Step 2: Read initial values from config snapshot**

```rust
let cfg = config.current();
let mut tick_secs = cfg.timing.scheduler_tick_seconds;
let workspace = cfg.workspace.clone();
```

- [ ] **Step 3: Add config change branch to select loop**

Add a new branch to the existing `tokio::select!`:

```rust
_ = config.changed() => {
    let cfg = config.current();
    let new_tick = cfg.timing.scheduler_tick_seconds;
    if new_tick != tick_secs {
        tick_secs = new_tick;
        interval = tokio::time::interval(Duration::from_secs(tick_secs));
        info!(tick_seconds = tick_secs, "scheduler tick interval updated");
    }
    // Reload agent entries in case workspace agents changed
    let (new_scheduled, new_idle) = build_entries(&workspace);
    scheduled = new_scheduled;
    idle_agents = new_idle;
}
```

- [ ] **Step 4: Run `just ci` and fix any issues**

- [ ] **Step 5: Commit**

```
feat: scheduler reacts to config reload for tick interval changes
```

---

## Task 5: Migrate Watcher + Reconciliation Loop

**Files:**

- Modify: `src/daemon/watcher.rs`

Both `spawn_watcher` and `spawn_reconciliation_loop` currently take `workspace: PathBuf`
and `embeddings_config: EmbeddingsConfig` separately. Change to `SharedConfig`.

- [ ] **Step 1: Migrate `spawn_watcher`**

Change signature:

```rust
pub fn spawn_watcher(
    db: GhostDb,
    config: SharedConfig,  // replaces workspace + embeddings_config
    mut shutdown: tokio::sync::watch::Receiver<bool>,
    watcher_busy: Arc<AtomicBool>,
) -> tokio::task::JoinHandle<()> {
```

Inside, read initial values:

```rust
let cfg = config.current();
let workspace = cfg.workspace.clone();
let client = EmbeddingClient::new(&cfg.embeddings);
```

The workspace is immutable so it's fine to read once. The EmbeddingClient is created
from embeddings config — if embeddings URL/model change, the watcher won't pick that up
until restart. This is acceptable per the spec caveat. (Re-creating the client on each
batch would work too but is unnecessary complexity.)

- [ ] **Step 2: Migrate `spawn_reconciliation_loop`**

Same pattern — change signature to take `SharedConfig`, read initial values at start.

- [ ] **Step 3: Run `just ci` and fix any issues**

- [ ] **Step 4: Commit**

```
refactor: migrate watcher and reconciliation loop to SharedConfig
```

---

## Task 6: Migrate Discord Handler + Wire DaemonHandle + SIGHUP handler

**Files:**

- Modify: `src/interfaces/discord/bot.rs`
- Modify: `src/interfaces/discord/start.rs`
- Modify: `src/daemon/run.rs`

These are merged because `start_discord`'s signature change requires updating its call
site in `run.rs`, and `run.rs` is where the watch channel is created. Doing them
together avoids a non-compiling intermediate state.

- [ ] **Step 1: Change Discord Handler field**

In `bot.rs`, change `config: Config` to `config: SharedConfig`. Update `Handler::new()`
signature. Update `self.config.workspace` to `self.config.current().workspace`.

Also update how `allowed_user_ids` is read — currently stored as a separate
`Vec<String>` field extracted from config at construction. For hot-reload, read it from
`.current().discord.allowed_user_ids` in the auth check instead of caching it. This
makes `allowed_user_ids` hot-reloadable. Remove the `allowed_user_ids` field and
constructor parameter.

- [ ] **Step 2: Update start_discord**

In `src/interfaces/discord/start.rs`, change `start_discord` to take `&SharedConfig`
instead of `&Config`. Read initial config for the enabled/token checks:

```rust
let cfg = config.current();
if !cfg.discord.enabled { ... }
if cfg.discord.allowed_user_ids.is_empty() { ... }
```

Pass `config.clone()` (SharedConfig clone, cheap) to `Handler::new()`.

- [ ] **Step 3: Update DaemonHandle**

In `src/daemon/run.rs`, add/change fields:

```rust
pub struct DaemonHandle {
    // existing fields...
    pub config: SharedConfig,          // was: config: Config
    config_tx: ConfigSender,           // new
}
```

- [ ] **Step 4: Create watch channel in `boot_with_config`**

At the top of `boot_with_config`:

```rust
let (config_tx, config_rx) = tokio::sync::watch::channel(Arc::new(config));
```

Then use `config_rx.current()` for boot-time reads and `config_rx.clone()` for
distributing to consumers (AgentRunner, SessionChat, Scheduler, Watcher,
ReconciliationLoop, Discord). Update `handle_bundled_updates` and other functions that
take `&Config` to read from the SharedConfig.

- [ ] **Step 5: Add SIGHUP handler to `run()`**

Replace `shutdown_signal().await;` with a loop:

```rust
let mut sighup = tokio::signal::unix::signal(
    tokio::signal::unix::SignalKind::hangup()
).expect("failed to register SIGHUP handler");

loop {
    tokio::select! {
        _ = shutdown_signal() => break,
        _ = sighup.recv() => {
            info!("SIGHUP received, reloading config...");
            match crate::config::reload() {
                Ok(new_config) => {
                    let current = handle.config.current();
                    match crate::config::validate_reload(&current, &new_config) {
                        Ok(()) => {
                            handle.config_tx.send(Arc::new(new_config)).ok();
                            info!("config reloaded successfully");
                        }
                        Err(e) => {
                            logfire::warn!(
                                "config reload rejected",
                                error = e.to_string(),
                            );
                        }
                    }
                }
                Err(e) => {
                    logfire::warn!(
                        "config reload failed",
                        error = e.to_string(),
                    );
                }
            }
        }
    }
}
```

- [ ] **Step 6: Run `just ci` and fix all issues**

This is the biggest integration point. Expect compiler errors from mismatched types
throughout `run.rs`. Fix each one by reading from the SharedConfig.

- [ ] **Step 7: Commit**

```
feat: migrate Discord handler, wire SharedConfig through daemon, add SIGHUP handler
```

---

## Task 7: CLI `ghost config reload` command

**Files:**

- Create: `src/cli/reload.rs`
- Modify: `src/cli/config.rs`
- Modify: `src/cli/mod.rs`

- [ ] **Step 1: Create `src/cli/reload.rs`**

```rust
use crate::error::GhostError;

/// Validate the current config and signal the running daemon to reload.
pub fn execute() -> Result<(), GhostError> {
    // Step 1: Validate the new config
    crate::config::load_dotenv_from_config_dir();
    match crate::config::load() {
        Ok(_) => {}
        Err(e) => {
            eprintln!("Config validation failed:\n  {e}");
            std::process::exit(1);
        }
    }

    // Step 2: Send SIGHUP via service manager
    if cfg!(target_os = "macos") {
        let uid = std::process::Command::new("id")
            .arg("-u")
            .output()
            .map_err(|e| {
                std::io::Error::new(e.kind(), format!("failed to run id: {e}"))
            })?;
        let uid = String::from_utf8_lossy(&uid.stdout).trim().to_string();

        let status = std::process::Command::new("launchctl")
            .args(["kill", "SIGHUP", &format!("gui/{uid}/com.ghost.daemon")])
            .status()
            .map_err(|e| {
                std::io::Error::new(e.kind(), format!("failed to run launchctl: {e}"))
            })?;

        if !status.success() {
            return Err(std::io::Error::other(
                "launchctl kill SIGHUP failed — is the daemon running?"
            ).into());
        }
    } else {
        let status = std::process::Command::new("systemctl")
            .args(["--user", "kill", "--signal=SIGHUP", "ghost-daemon"])
            .status()
            .map_err(|e| {
                std::io::Error::new(e.kind(), format!("failed to run systemctl: {e}"))
            })?;

        if !status.success() {
            return Err(std::io::Error::other(
                "systemctl kill SIGHUP failed — is the daemon running?"
            ).into());
        }
    }

    println!("Config reloaded successfully.");
    Ok(())
}
```

- [ ] **Step 2: Add `Reload` to ConfigCommand**

In `src/cli/config.rs`:

```rust
#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    Get { key: String },
    Set { key: String, value: String },
    /// Reload config without restarting the daemon
    Reload,
}
```

And in the `execute` match:

```rust
ConfigCommand::Reload => crate::cli::reload::execute(),
```

- [ ] **Step 3: Add `pub mod reload;` to `src/cli/mod.rs`**

- [ ] **Step 4: Run `just ci` and fix any issues**

- [ ] **Step 5: Commit**

```
feat: add `ghost config reload` CLI command
```

---

## Task 8: Update existing "ghost reboot" references

**Files:**

- Modify: `src/cli/browsers.rs`
- Modify: `assets/skills/browser-use/skill.md`

- [ ] **Step 1: Update `src/cli/browsers.rs`**

Line 84: `"Run `ghost reboot` to apply changes."` → `"Run `ghost config
reload` to apply changes."`

Line 92: Same change.

Line 273: `"  ghost reboot"` → `"  ghost config reload"`

- [ ] **Step 2: Update `assets/skills/browser-use/skill.md`**

Line 154: `"3. Tell the OPERATOR to run `ghost reboot` to pick up the new config."` →
`"3. Tell the OPERATOR to run `ghost config reload` to pick up the new config."`

Also update line 155 onwards — the paragraph after says "After reboot, the OPERATOR's
browser is available by name." Change "After reboot" to "After reload".

- [ ] **Step 3: Run `just ci` and fix any issues**

- [ ] **Step 4: Commit**

```
docs: update "ghost reboot" references to "ghost config reload" where appropriate
```

---

## Task 9: Documentation

**Files:**

- Modify: docs site pages for CLI reference (check `docs/src/content/docs/`)

Read the `/docs` skill before making changes. Document `ghost config reload` in the
appropriate docs page (likely the CLI reference or configuration guide). Cover:

- What it does (validates + signals daemon)
- What's hot-reloadable vs requires restart
- The embeddings model caveat
- Example usage

- [ ] **Step 1: Read the `/docs` skill for conventions**

- [ ] **Step 2: Find the right docs page and add the section**

- [ ] **Step 3: Run `just doc` to verify docs build**

- [ ] **Step 4: Commit**

```
docs: document ghost config reload command
```

---

## Task 10: Final verification

- [ ] **Step 1: Run `just ci`** — all tests pass, no warnings

- [ ] **Step 2: Manual smoke test** (if daemon is running)

1. `ghost config get models.default` — note current value
2. Edit `config.toml` to change a hot-reloadable field (e.g. `debug.save_requests`)
3. `ghost config reload` — should print success
4. Verify the daemon log shows "config reloaded successfully"

- [ ] **Step 3: Test validation error path**

1. Introduce a syntax error in `config.toml`
2. `ghost config reload` — should print the parse error and exit 1
3. Fix the syntax error

- [ ] **Step 4: Test immutable field rejection**

1. Change `workspace` in `config.toml`
2. `ghost config reload` — should succeed at CLI validation (workspace is valid) but
   daemon should log "config reload rejected: cannot change 'workspace'"
3. Revert the change
