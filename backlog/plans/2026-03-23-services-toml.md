# services.toml + ghost services + ghost start/stop — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or superpowers:executing-plans
> to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a generic TOML-based service registry (`services.toml`) with CLI commands
to manage, start, stop, and update all GHOST services as a unified stack.

**Architecture:** A new `src/services.rs` module owns the `ServiceRegistry` type (parse,
query, mutate, run commands). CLI commands in `src/cli/services.rs` and
`src/cli/start_stop.rs` are thin wrappers that load the registry and call its methods.
`ghost init` generates the file. `ghost reset` reads it for shutdown.

**Tech Stack:** Rust, toml/toml_edit (parse/preserve formatting), clap (CLI),
std::process::Command (execute service commands).

**Spec:** `backlog/tasks/4-easy-install/7-ghost-update-services.md`

**Testing skill:** Read `/testing` before writing any test.

---

## File Structure

### New files

| File                    | Responsibility                                                |
| ----------------------- | ------------------------------------------------------------- |
| `src/services.rs`       | `ServiceEntry`, `ServiceRegistry` — parse, query, mutate, run |
| `src/cli/services.rs`   | `ghost services list/add/remove/update/status` subcommands    |
| `src/cli/start_stop.rs` | `ghost start` and `ghost stop` commands                       |

### Modified files

| File                               | Changes                                                          |
| ---------------------------------- | ---------------------------------------------------------------- |
| `src/main.rs`                      | Add `Services`, `Start`, `Stop` to `Commands` enum + dispatch    |
| `src/cli/mod.rs`                   | Add `pub mod services;` and `pub mod start_stop;`                |
| `src/lib.rs`                       | Add `pub mod services;`                                          |
| `src/onboarding/wizard.rs:196-200` | Call `generate_services_toml()` after compose + service files    |
| `src/cli/reset.rs:63-91`           | Replace hardcoded `stop_services()` with registry-based shutdown |
| `assets/skills/services/skill.md`  | Rewrite: document CLI commands, remove redundant manual examples |

---

## Task 1: ServiceRegistry core — parse and query

**Files:**

- Create: `src/services.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Write the data types and parser**

```rust
// src/services.rs
use std::path::Path;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// A single service entry from services.toml.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceEntry {
    pub start: Option<String>,
    pub stop: Option<String>,
    pub update: Option<String>,
    pub status: Option<String>,
}

/// Ordered collection of service entries.
/// IndexMap preserves TOML insertion order (required for top-to-bottom execution).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ServiceRegistry {
    #[serde(flatten)]
    pub entries: IndexMap<String, ServiceEntry>,
}

impl ServiceRegistry {
    /// Load from a services.toml file. Returns error if file is missing or malformed.
    pub fn load(path: &Path) -> Result<Self, ServiceRegistryError> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| ServiceRegistryError::Io(path.to_path_buf(), e))?;
        toml::from_str(&content)
            .map_err(|e| ServiceRegistryError::Parse(path.to_path_buf(), e))
    }

    /// Load from file, returning an empty registry if the file doesn't exist.
    pub fn load_or_empty(path: &Path) -> Result<Self, ServiceRegistryError> {
        if !path.exists() {
            return Ok(Self::default());
        }
        Self::load(path)
    }

    /// Entry names in file order.
    pub fn names(&self) -> Vec<&str> {
        self.entries.keys().map(|s| s.as_str()).collect()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ServiceRegistryError {
    #[error("cannot read {0}: {1}")]
    Io(std::path::PathBuf, std::io::Error),
    #[error("invalid TOML in {0}: {1}")]
    Parse(std::path::PathBuf, toml::de::Error),
    #[error("service '{0}' already exists")]
    AlreadyExists(String),
    #[error("service '{0}' not found")]
    NotFound(String),
    #[error("at least one command field is required")]
    EmptyEntry,
    #[error("{service}: command failed (exit {code})\n{stderr}")]
    CommandFailed {
        service: String,
        code: i32,
        stderr: String,
    },
}
```

- [ ] **Step 2: Add `pub mod services;` to `src/lib.rs`**

- [ ] **Step 3: Write tests for parse and query**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_toml(content: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f
    }

    #[test]
    fn parse_full_entry() {
        let f = write_toml(r#"
[containers]
start = "podman compose up -d"
stop = "podman compose down"
update = "podman compose pull"
status = "podman compose ps"
"#);
        let reg = ServiceRegistry::load(f.path()).unwrap();
        assert_eq!(reg.names(), vec!["containers"]);
        let e = &reg.entries["containers"];
        assert_eq!(e.start.as_deref(), Some("podman compose up -d"));
        assert_eq!(e.stop.as_deref(), Some("podman compose down"));
    }

    #[test]
    fn parse_partial_entry() {
        let f = write_toml(r#"
[docling]
start = "systemctl --user start docling-serve"
stop = "systemctl --user stop docling-serve"
"#);
        let reg = ServiceRegistry::load(f.path()).unwrap();
        let e = &reg.entries["docling"];
        assert!(e.update.is_none());
        assert!(e.status.is_none());
    }

    #[test]
    fn parse_preserves_order() {
        let f = write_toml(r#"
[containers]
start = "a"

[llama-server]
start = "b"

[docling]
start = "c"
"#);
        let reg = ServiceRegistry::load(f.path()).unwrap();
        assert_eq!(reg.names(), vec!["containers", "llama-server", "docling"]);
    }

    #[test]
    fn load_missing_file_errors() {
        let result = ServiceRegistry::load(Path::new("/nonexistent/services.toml"));
        assert!(result.is_err());
    }

    #[test]
    fn load_or_empty_missing_file() {
        let reg = ServiceRegistry::load_or_empty(Path::new("/nonexistent/services.toml")).unwrap();
        assert!(reg.entries.is_empty());
    }

    #[test]
    fn parse_malformed_toml() {
        let f = write_toml("not valid [[ toml");
        assert!(ServiceRegistry::load(f.path()).is_err());
    }
}
```

- [ ] **Step 4: Add `indexmap` dependency**

```toml
# Cargo.toml
indexmap = { version = "2", features = ["serde"] }
```

- [ ] **Step 5: Run tests**

Run: `cargo test --lib services -- --nocapture` Expected: All pass (IndexMap preserves
insertion order).

- [ ] **Step 6: Commit**

```bash
git add src/services.rs src/lib.rs Cargo.toml Cargo.lock
git commit -m "feat: add ServiceRegistry type for services.toml parsing"
```

---

## Task 2: ServiceRegistry mutations — add, remove, save

**Files:**

- Modify: `src/services.rs`

- [ ] **Step 1: Add mutation methods**

```rust
impl ServiceRegistry {
    /// Add a new service entry. Errors if name already exists.
    pub fn add(
        &mut self,
        name: String,
        entry: ServiceEntry,
    ) -> Result<(), ServiceRegistryError> {
        if entry.start.is_none()
            && entry.stop.is_none()
            && entry.update.is_none()
            && entry.status.is_none()
        {
            return Err(ServiceRegistryError::EmptyEntry);
        }
        if self.entries.contains_key(&name) {
            return Err(ServiceRegistryError::AlreadyExists(name));
        }
        self.entries.insert(name, entry);
        Ok(())
    }

    /// Remove a service entry by name. Errors if not found.
    pub fn remove(&mut self, name: &str) -> Result<(), ServiceRegistryError> {
        if self.entries.shift_remove(name).is_none() {
            return Err(ServiceRegistryError::NotFound(name.to_string()));
        }
        Ok(())
    }

    /// Write the registry back to a file.
    pub fn save(&self, path: &Path) -> Result<(), ServiceRegistryError> {
        let content = toml::to_string_pretty(self)
            .map_err(|e| ServiceRegistryError::Parse(
                path.to_path_buf(),
                // toml::ser::Error doesn't implement Into<toml::de::Error>,
                // so use a new variant or convert. Simplest: add a Serialize variant.
                // For now, just use the Io variant with a synthetic error.
                // Actually — add a new error variant.
            ))?;
        std::fs::write(path, content)
            .map_err(|e| ServiceRegistryError::Io(path.to_path_buf(), e))
    }
}
```

Add a `Serialize` error variant to `ServiceRegistryError`:

```rust
#[error("cannot serialize services: {0}")]
Serialize(#[from] toml::ser::Error),
```

And fix `save` to use it:

```rust
pub fn save(&self, path: &Path) -> Result<(), ServiceRegistryError> {
    let content = toml::to_string_pretty(self)?;
    std::fs::write(path, content)
        .map_err(|e| ServiceRegistryError::Io(path.to_path_buf(), e))
}
```

- [ ] **Step 2: Write tests**

```rust
#[test]
fn add_and_remove() {
    let mut reg = ServiceRegistry::default();
    reg.add("foo".into(), ServiceEntry {
        start: Some("start-foo".into()),
        stop: None, update: None, status: None,
    }).unwrap();
    assert_eq!(reg.names(), vec!["foo"]);

    reg.remove("foo").unwrap();
    assert!(reg.entries.is_empty());
}

#[test]
fn add_duplicate_errors() {
    let mut reg = ServiceRegistry::default();
    reg.add("foo".into(), ServiceEntry {
        start: Some("x".into()), stop: None, update: None, status: None,
    }).unwrap();
    assert!(reg.add("foo".into(), ServiceEntry {
        start: Some("y".into()), stop: None, update: None, status: None,
    }).is_err());
}

#[test]
fn remove_missing_errors() {
    let mut reg = ServiceRegistry::default();
    assert!(reg.remove("nope").is_err());
}

#[test]
fn add_empty_entry_errors() {
    let mut reg = ServiceRegistry::default();
    assert!(reg.add("foo".into(), ServiceEntry {
        start: None, stop: None, update: None, status: None,
    }).is_err());
}

#[test]
fn save_roundtrip() {
    let mut reg = ServiceRegistry::default();
    reg.add("svc".into(), ServiceEntry {
        start: Some("start-cmd".into()),
        stop: Some("stop-cmd".into()),
        update: None,
        status: None,
    }).unwrap();

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("services.toml");
    reg.save(&path).unwrap();

    let loaded = ServiceRegistry::load(&path).unwrap();
    assert_eq!(loaded.entries["svc"].start.as_deref(), Some("start-cmd"));
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test --lib services -- --nocapture` Expected: All pass.

- [ ] **Step 4: Commit**

```bash
git add src/services.rs
git commit -m "feat: add ServiceRegistry add/remove/save mutations"
```

---

## Task 3: ServiceRegistry command execution

**Files:**

- Modify: `src/services.rs`

- [ ] **Step 1: Add command runner**

```rust
use std::process::Command;

/// Which command field to run.
#[derive(Debug, Clone, Copy)]
pub enum ServiceField {
    Start,
    Stop,
    Update,
    Status,
}

impl ServiceField {
    fn get(self, entry: &ServiceEntry) -> &Option<String> {
        match self {
            Self::Start => &entry.start,
            Self::Stop => &entry.stop,
            Self::Update => &entry.update,
            Self::Status => &entry.status,
        }
    }
}

/// Result of running a service command.
#[derive(Debug)]
pub struct RunResult {
    pub service: String,
    pub success: bool,
    pub output: String,
}

impl ServiceRegistry {
    /// Run a field for each entry.
    /// `stop_on_failure` — if true, abort on first failure.
    /// `reverse` — if true, iterate entries bottom-to-top.
    pub fn run_field(
        &self,
        field: ServiceField,
        stop_on_failure: bool,
        reverse: bool,
    ) -> Vec<RunResult> {
        let entries: Vec<_> = self.entries.iter().collect();
        let iter: Box<dyn Iterator<Item = &(&String, &ServiceEntry)>> = if reverse {
            Box::new(entries.iter().rev())
        } else {
            Box::new(entries.iter())
        };

        let mut results = Vec::new();
        for (name, entry) in iter {
            let cmd = field.get(entry);

            let Some(cmd) = cmd else {
                continue;
            };

            let result = run_shell_command(name, cmd);
            let failed = !result.success;
            results.push(result);

            if failed && stop_on_failure {
                break;
            }
        }
        results
    }
}

fn run_shell_command(service: &str, cmd: &str) -> RunResult {
    let output = Command::new("sh")
        .args(["-c", cmd])
        .output();

    match output {
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            let stderr = String::from_utf8_lossy(&o.stderr);
            let combined = if stderr.is_empty() {
                stdout.to_string()
            } else {
                format!("{stdout}{stderr}")
            };
            RunResult {
                service: service.to_string(),
                success: o.status.success(),
                output: combined.trim().to_string(),
            }
        }
        Err(e) => RunResult {
            service: service.to_string(),
            success: false,
            output: format!("failed to execute: {e}"),
        },
    }
}
```

- [ ] **Step 2: Run existing tests to make sure nothing broke**

Run: `cargo test --lib services -- --nocapture`

- [ ] **Step 3: Commit**

```bash
git add src/services.rs
git commit -m "feat: add ServiceRegistry command runner"
```

---

## Task 4: ghost services CLI commands

**Files:**

- Create: `src/cli/services.rs`
- Modify: `src/cli/mod.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Create the CLI module**

```rust
// src/cli/services.rs
use clap::Subcommand;

use crate::error::GhostError;
use crate::services::{ServiceEntry, ServiceRegistry};

#[derive(Debug, Subcommand)]
pub enum ServicesCommand {
    /// List all registered services
    List,
    /// Add a service entry
    Add {
        /// Service name
        #[arg(long)]
        name: String,
        /// Start command
        #[arg(long)]
        start: Option<String>,
        /// Stop command
        #[arg(long)]
        stop: Option<String>,
        /// Update command
        #[arg(long)]
        update: Option<String>,
        /// Status command
        #[arg(long)]
        status: Option<String>,
    },
    /// Remove a service entry
    Remove {
        /// Service name
        name: String,
    },
    /// Run update commands for all services
    Update,
    /// Check status of all services
    Status,
}

pub async fn execute(command: ServicesCommand) -> Result<(), GhostError> {
    match command {
        ServicesCommand::List => execute_list(),
        ServicesCommand::Add {
            name, start, stop, update, status,
        } => execute_add(name, start, stop, update, status),
        ServicesCommand::Remove { name } => execute_remove(name),
        ServicesCommand::Update => execute_update(),
        ServicesCommand::Status => execute_status(),
    }
}

fn services_toml_path() -> Result<std::path::PathBuf, GhostError> {
    let config = crate::config::load()?;
    Ok(config.workspace.join("services/services.toml"))
}

fn execute_list() -> Result<(), GhostError> {
    let path = services_toml_path()?;
    let reg = ServiceRegistry::load(&path)
        .map_err(GhostError::from)?;

    if reg.entries.is_empty() {
        println!("No services registered.");
        return Ok(());
    }

    // Header
    println!(
        "{:<20} {:<6} {:<6} {:<6} {:<6}",
        "NAME", "START", "STOP", "UPDATE", "STATUS"
    );
    println!("{}", "-".repeat(50));

    for (name, entry) in &reg.entries {
        let check = |f: &Option<String>| if f.is_some() { "✓" } else { "-" };
        println!(
            "{:<20} {:<6} {:<6} {:<6} {:<6}",
            name,
            check(&entry.start),
            check(&entry.stop),
            check(&entry.update),
            check(&entry.status),
        );
    }
    Ok(())
}

fn execute_add(
    name: String,
    start: Option<String>,
    stop: Option<String>,
    update: Option<String>,
    status: Option<String>,
) -> Result<(), GhostError> {
    let path = services_toml_path()?;
    let mut reg = ServiceRegistry::load_or_empty(&path)
        .map_err(GhostError::from)?;

    reg.add(name.clone(), ServiceEntry { start, stop, update, status })
        .map_err(GhostError::from)?;

    reg.save(&path)
        .map_err(GhostError::from)?;

    println!("Added service '{name}'");
    Ok(())
}

fn execute_remove(name: String) -> Result<(), GhostError> {
    let path = services_toml_path()?;
    let mut reg = ServiceRegistry::load(&path)
        .map_err(GhostError::from)?;

    reg.remove(&name)
        .map_err(GhostError::from)?;

    reg.save(&path)
        .map_err(GhostError::from)?;

    println!("Removed service '{name}'");
    Ok(())
}

fn execute_update() -> Result<(), GhostError> {
    let path = services_toml_path()?;
    let reg = ServiceRegistry::load(&path)
        .map_err(GhostError::from)?;

    let results = reg.run_field(ServiceField::Update, true, false);

    if results.is_empty() {
        println!("No services have an update command.");
        return Ok(());
    }

    for r in &results {
        if r.success {
            println!("  ✓ {}", r.service);
        } else {
            eprintln!("  ✗ {}: {}", r.service, r.output);
            return Err(GhostError::Other(
                format!("update failed for '{}'", r.service).into(),
            ));
        }
    }

    println!("All services updated.");
    Ok(())
}

fn execute_status() -> Result<(), GhostError> {
    let path = services_toml_path()?;
    let reg = ServiceRegistry::load(&path)
        .map_err(GhostError::from)?;

    let results = reg.run_field(ServiceField::Status, false, false);

    if results.is_empty() {
        println!("No services have a status command.");
        return Ok(());
    }

    for r in &results {
        if r.success {
            println!("  ✓ {}", r.service);
        } else {
            println!("  ✗ {}", r.service);
        }
    }
    Ok(())
}
```

- [ ] **Step 2: Add `pub mod services;` to `src/cli/mod.rs`**

- [ ] **Step 3: Wire into `src/main.rs`**

Add to the `Commands` enum:

```rust
/// Manage registered services
Services {
    #[command(subcommand)]
    command: ghost::cli::services::ServicesCommand,
},
```

Add to the `dispatch` match:

```rust
Commands::Services { command } => ghost::cli::services::execute(command).await,
```

- [ ] **Step 4: Add `ServiceRegistryError` variant to `GhostError`**

In `src/error.rs`, add a variant so the CLI can use `?` directly:

```rust
#[error(transparent)]
ServiceRegistry(#[from] crate::services::ServiceRegistryError),
```

- [ ] **Step 5: Verify it compiles**

Run: `cargo check`

- [ ] **Step 6: Commit**

```bash
git add src/cli/services.rs src/cli/mod.rs src/main.rs src/error.rs
git commit -m "feat: add ghost services CLI commands"
```

---

## Task 5: ghost start / ghost stop

**Files:**

- Create: `src/cli/start_stop.rs`
- Modify: `src/cli/mod.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Create start/stop module**

Key behavior:

- `ghost start`: run service `start` commands (top-to-bottom, **stop on first
  failure**), then start daemon via service manager, then call
  `crate::cli::status::execute()`.
- `ghost stop`: stop daemon via service manager, then run service `stop` commands
  (bottom-to-top, **best-effort — don't stop on failure**), then call
  `crate::cli::status::execute()`.
- Missing `services.toml` → skip service commands, just control the daemon.

Platform-specific daemon control (reference `src/cli/reset.rs:94-127`):

**Linux (systemd):**

```rust
// Start
Command::new("systemctl").args(["--user", "start", "ghost-daemon"]).status()
// Stop
Command::new("systemctl").args(["--user", "disable", "--now", "ghost-daemon"]).status()
```

**macOS (launchd):**

```rust
// Get UID first
let uid = Command::new("id").arg("-u").output()...;

// Start — bootstrap loads the plist and starts the service
let plist = dirs::home_dir().unwrap().join("Library/LaunchAgents/com.ghost.daemon.plist");
Command::new("launchctl")
    .args(["bootstrap", &format!("gui/{uid}"), &plist.display().to_string()])
    .status()

// Stop — bootout unloads and stops
Command::new("launchctl")
    .args(["bootout", &format!("gui/{uid}/com.ghost.daemon")])
    .status()
```

Note: `bootstrap`/`bootout` (used here) are different from `kickstart` (used in
`reboot.rs`). `bootstrap` loads a plist file, `bootout` unloads by label, `kickstart`
restarts an already-loaded service.

- [ ] **Step 2: Add `pub mod start_stop;` to `src/cli/mod.rs`**

- [ ] **Step 3: Wire into `src/main.rs`**

Add to `Commands` enum:

```rust
/// Start all services and the daemon
Start,
/// Stop the daemon and all services
Stop,
```

Add to `dispatch`:

```rust
Commands::Start => ghost::cli::start_stop::execute_start().await,
Commands::Stop => ghost::cli::start_stop::execute_stop().await,
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo check`

- [ ] **Step 5: Commit**

```bash
git add src/cli/start_stop.rs src/cli/mod.rs src/main.rs
git commit -m "feat: add ghost start/stop commands"
```

---

## Task 6: ghost init — generate services.toml

**Files:**

- Modify: `src/onboarding/wizard.rs`

- [ ] **Step 1: Add services.toml generation function**

Add a function in `src/onboarding/wizard.rs` (or a helper in
`src/onboarding/services.rs`) that takes the `OnboardingState`, `detect::Platform`,
workspace path, and container runtime, and builds a `ServiceRegistry` with the correct
entries.

The function should:

- Check which services were selected (not skipped or remote-only)
- For container services (searxng, crawl4ai, docling container): add a single
  `[containers]` entry with compose start/stop/update commands using the detected
  runtime (`podman compose` or `docker compose`) and absolute path to compose file
- For native services (llama-server nix, docling nix): add individual entries with
  platform-specific start/stop/status/update commands
- Use absolute paths — no `$WORKSPACE` variables

- [ ] **Step 2: Call it in the wizard after service file installation**

In `wizard.rs`, after `install_service_files()` (around line 200), call the generator
and write `services.toml` to `$WORKSPACE/services/services.toml`.

Note: the init launch phase (line 205, `health::start_all_services()`) continues to use
its own hardcoded startup logic. This is intentional — during init, the wizard already
knows exactly what to start and has the container runtime reference. Migrating the init
launch phase to use `services.toml` is deferred.

- [ ] **Step 3: Verify it compiles**

Run: `cargo check`

- [ ] **Step 4: Manual test**

Run `ghost init` (with flags for speed) and verify `services.toml` is generated with
correct platform-specific commands.

- [ ] **Step 5: Commit**

```bash
git add src/onboarding/wizard.rs src/onboarding/services.rs
git commit -m "feat: generate services.toml during ghost init"
```

---

## Task 7: ghost reset — use services.toml for shutdown

**Files:**

- Modify: `src/cli/reset.rs`

- [ ] **Step 1: Replace hardcoded stop with registry-based shutdown**

In `reset.rs`, modify `stop_services()` to:

1. Try to load `services.toml` from the workspace
2. If found: use `reg.run_field(ServiceField::Stop, false, true)` (best-effort, reverse
   order)
3. If not found: fall back to the current hardcoded behavior (for backwards compat)

Keep the existing hardcoded functions as the fallback path.

- [ ] **Step 2: Verify it compiles**

Run: `cargo check`

- [ ] **Step 3: Commit**

```bash
git add src/cli/reset.rs
git commit -m "refactor: ghost reset uses services.toml for shutdown"
```

---

## Task 8: Update services skill

**Files:**

- Modify: `assets/skills/services/skill.md`

- [ ] **Step 1: Rewrite the skill**

Replace the bulk of the manual systemctl/launchctl/compose examples with the new CLI
commands. Keep it short and concise per the spec. Structure:

1. Architecture overview (brief — native vs container tiers)
2. CLI commands: `ghost start`, `ghost stop`,
   `ghost services list/add/remove/update/status`
3. Health checks: `ghost status` for HTTP probes, `ghost services status` for process
   checks
4. Troubleshooting: keep port conflict and log viewing sections (these are still
   useful), trim everything that's now covered by CLI commands
5. Optional extras: keep links to observability.md and tailscale.md

- [ ] **Step 2: Commit**

```bash
git add assets/skills/services/skill.md
git commit -m "docs: update services skill with new CLI commands"
```

---

## Task 9: Run CI

- [ ] **Step 1: Run full CI**

Run: `just ci` Expected: All checks pass (fmt, check, clippy, tests).

- [ ] **Step 2: Fix any issues**

- [ ] **Step 3: Final commit if needed**
