# 01 — Project Scaffolding and CLI Skeleton

## Overview

Set up the Rust project as a single crate with a CLI entry point using `clap`. The
binary is called `ghost` and exposes subcommands for all operations.

## CLI Subcommands

```
ghost daemon                # Start the daemon (Discord bot + job scheduler)
ghost init                  # Bootstrap workspace (first run)
ghost config get <key>
ghost config set <key> <value>
ghost auth codex            # OpenAI OAuth flow (see 05b)
ghost auth status           # Show authenticated providers
ghost auth revoke           # Delete stored tokens
ghost job list
ghost job validate <path>
ghost job run <name>        # Run a job manually (outside scheduler)
ghost job logs [name]
ghost session list
ghost session show <id>
ghost knowledge search <query>
ghost knowledge get <id>
ghost knowledge reindex     # Rebuild all embeddings (see 14)
ghost version
```

The daemon is the long-running process. Other subcommands are one-shot operations that
read/write the database and workspace directly. They do NOT communicate with the daemon
via IPC — they access the same SurrealDB and filesystem directly.

> If a command needs to interact with a running daemon (e.g., send a message as the
> GHOST), we'll add that later. For now, CLI commands are independent.

## Project Structure

```
Cargo.toml
src/
├── main.rs           # clap CLI dispatch
├── cli/
│   ├── mod.rs        # re-exports
│   ├── daemon.rs     # ghost daemon (thin — delegates to daemon::run())
│   ├── init.rs       # ghost init
│   ├── config.rs     # ghost config get/set
│   ├── auth.rs       # ghost auth codex/status/revoke
│   ├── job.rs        # ghost job list/validate/run/logs
│   ├── session.rs    # ghost session list/show
│   └── knowledge.rs  # ghost knowledge search/get/reindex
├── daemon/
│   └── mod.rs        # Subsystem wiring, task spawning, signal handling
├── config/
│   └── mod.rs        # (placeholder)
└── error.rs          # Top-level GhostError type
```

## Key Dependencies (Cargo.toml)

```toml
[package]
name = "ghost"
version = "0.1.0"
edition = "2024"

[dependencies]
clap = { version = "4", features = ["derive"] }
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
logfire = "0.6"
chrono = { version = "0.4", features = ["serde"] }

[features]
live-tests = []
```

## Error Type

```rust
#[derive(Debug, thiserror::Error)]
pub enum GhostError {
    #[error("config error: {0}")]
    Config(String),

    #[error("database error: {0}")]
    Database(String),

    #[error("provider error: {0}")]
    Provider(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
```

This will grow as features are added. Each module may define its own error type that
converts into `GhostError`.

## Validation

1. `cargo run -- --help` — all subcommands are listed (daemon, job, auth, init,
   knowledge)
2. `cargo run -- daemon` — prints a stub message or exits cleanly
3. `cargo run -- job list` — prints a stub message
4. `cargo run -- auth status` — prints a stub message
5. `just ci` — passes with no warnings

## Acceptance Criteria

- `cargo build` produces a `ghost` binary
- `ghost --help` shows all subcommands
- `ghost version` prints version
- All other subcommands exist as stubs that print "not yet implemented"
- `just ci` passes
