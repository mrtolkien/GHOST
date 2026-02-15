# GHOST — CLAUDE.md

This file is the high-signal baseline for all agent runs. Feature-specific
implementation details live in `specs/`.

`AGENTS.md` is symlinked to this file.

## HOW TO WORK

ASK QUESTIONS ABOUT DESIGN. DON'T JUST START WRITING CODE.

You are _great_ at writing code, but _horrendous_ at designing systems and products. You
should make extremely few assumptions and regularly ask the user if your approach is
right, and your understanding of the product and features are right. DO NOT MAKE
ASSUMPTIONS ABOUT WHAT THE USER WANTS: ASK THEM.

Terminology rule for docs/prose: always write `GHOST` and `OPERATOR` in all caps. Keep
real code/file identifiers (crate names, paths, variable names) unchanged.

## CRUCIAL

Always update this file when:

- Project-wide core rules change
- Information here is stale
- An important assumption documented here is proven wrong

## Project Overview

GHOST is a personal AI agent platform. A single binary (`ghost`) runs one GHOST for one
OPERATOR. It provides persistent memory, background jobs, and multi-interface
communication (Discord as the primary interface).

### Predecessor

This project is a reboot of `../t-koma`. When implementing features, consult the old
codebase for implementation patterns and prior art — individual spec files in `specs/`
link to the relevant old code where useful. If you adapt behavior from t-koma, validate
with the user that the behavior should carry over (the reboot intentionally changes
several things).

### Architecture at a Glance

- Single binary, single crate
- One GHOST, one OPERATOR — identity lives in workspace files, not database tables
- SurrealDB (embedded) for storage — enables graph-based knowledge with typed edges
- Logfire for observability — mandatory tracing spans on all meaningful operations
- CLI-first — all features accessible through direct commands (`ghost daemon`,
  `ghost job validate`, etc.)
- Skills over tools — prefer agentskills.io skills + file reads over adding new tool APIs

## Core Concepts

- **GHOST**: The AI agent. Identity defined by workspace files (BOOT.md, SOUL.md,
  OPERATOR.md). One per installation.
- **OPERATOR**: The human user. Identified by Discord user ID in config. One per
  installation.
- **Session**: Chat thread between OPERATOR and GHOST.
- **Job**: Markdown file in `$WORKSPACE/jobs/` with TOML frontmatter. Cron-scheduled.
  Heartbeat and reflection are dedicated subsystems (not regular jobs).
- **Knowledge**: Notes, references, and diary entries stored in SurrealDB with typed graph
  edges and embeddings search.
- **Skill**: agentskills.io-compatible files in `$WORKSPACE/skills/`. Read via standard
  file tools, no dedicated `load_skill` tool.
- **Provider**: LLM backend (OpenRouter for PoC). Provider trait for future extensibility.

## Workspace and Flow

- Work in the currently opened workspace/worktree only.
- Do not jump to repo roots outside the opened workspace.
- Prefer small, atomic commits with conventional commit messages (`feat:`, `fix:`,
  `refactor:`, `test:`, `docs:`, `chore:`).

## Text-First Feature Design

- Prefer plain text files in the GHOST workspace as the primary feature surface (jobs,
  skills, identity files, knowledge notes).
- Prefer skills + CLI workflows over adding new tool APIs.
- Add a dedicated tool only when text + existing tools + CLI cannot deliver the feature
  safely or ergonomically.

## MCPs

Make extensive use of MCPs available to you:

- `context7` for up-to-date library documentation
- `rust-analyzer-mcp` for refactors and code actions
- `gh` for interacting with GitHub

Prefer MCP-backed answers over assumptions for library/framework behavior.

## Code Quality Rules

### Observability (NON-NEGOTIABLE)

Every meaningful public function MUST have a `tracing` span instrumented via
`#[instrument]` or `logfire::span!()`. This is the single most important code quality
rule. When there was a crash in the predecessor app, it was extremely hard to fix because
logs were lackluster and there was no tracing.

- Use `#[tracing::instrument(skip_all, fields(relevant_field = %value))]` on async
  functions
- Use `logfire::info!()`, `logfire::warn!()`, `logfire::error!()` for structured events
- Include relevant context in span fields (session ID, job name, provider, model, etc.)
- Log all external calls (provider API, Discord, embeddings) with timing
- Log all errors with full context before propagating
- Use `tracing::Span::current().record()` to add fields mid-execution when useful

### Error Handling

- Use `thiserror` for all error types — define domain-specific error enums
- No `.unwrap()` or `.expect()` in production code (tests are fine)
- Propagate errors with `?` — add context with `.map_err()` or a wrapper type
- Log errors at the boundary where they are handled, not where they are created
- Every error variant should carry enough context to diagnose the issue from logs alone

### Code Structure

- Single crate, organized with modules. No workspace.
- If you need over 4 levels of indentation, break it into functions.
- Avoid excess comments: code should be expressive and readable. If it requires comments,
  it likely needs a refactor.
- Break down complex systems into clear functions or traits, and if required, multiple
  files with clear names.
- A file over 500 LoC (excluding tests) likely means a design issue. Humans search code
  through filenames.
- Do not put logic in `mod.rs` files — they should be mostly barrel files (re-exports).

### Rust Style

- Use `clippy` with default lints at minimum. Address all warnings.
- Prefer `&str` over `String` in function parameters when ownership isn't needed.
- Prefer `impl Trait` in argument position for flexibility.
- Use `Arc` for shared state, not `Rc` (we are always async/multi-threaded).
- Derive `Debug` on all public types.
- Use `#[must_use]` on functions that return values that should not be ignored.

### Dependencies

Core stack (do not change without discussion):

- **Async runtime**: Tokio
- **HTTP framework**: Axum
- **HTTP client**: reqwest (rustls)
- **Database**: SurrealDB (embedded, surrealkv backend)
- **Discord**: serenity
- **Observability**: logfire + tracing
- **Error handling**: thiserror
- **Serialization**: serde + serde_json
- **Config**: toml
- **Time**: chrono
- **Embeddings**: Ollama (HTTP API)

## Development Flow

Iterate until all spec items are built and tested:

1. At each step:
   - Run `just check`
   - Run `just clippy`
   - Run `just test` (no live tests)
   - Run `just fmt`
2. Once an atomic feature is complete, make a conventional commit.
3. Offer the user to create a pull request with the `gh` MCP.

## Testing Strategy

- **Integration tests** with a `live-tests` feature flag for all major features.
- Maintain a robust, reusable integration test "starting state" (test fixtures) that can
  be shared across tests.
- **Unit tests** only when there are genuinely complex behaviors or external crate
  behaviors to validate.
- No snapshot testing (dropped insta).
- Live tests (`--features live-tests`) are human-run only.

## Configuration

- Config file: `~/.config/ghost/config.toml` (non-sensitive settings)
- Secrets: `.env` file or environment variables (API keys, tokens)
- Workspace: `~/GHOST/` by default, configurable in `config.toml`
- Path override: `GHOST_CONFIG_DIR` env var overrides config root
- The GHOST can modify config through CLI commands (to minimize hand-editing failures)

## Project Layout

```
src/
├── main.rs              # CLI entry point (clap)
├── cli/                 # CLI subcommands (daemon, chat, job, config, ...)
├── config/              # Config types, loading, defaults
├── db/                  # SurrealDB schema, queries, connection
├── providers/           # Provider trait + OpenRouter implementation
├── chat/                # Chat orchestration, session management, compaction
├── tools/               # Tool definitions and implementations
├── jobs/                # Job scheduling, triggers, execution
├── knowledge/           # Knowledge types, search, graph operations
├── discord/             # Discord bot transport
├── prompt/              # System prompt rendering
├── web/                 # Web search, web fetch, web cache
└── observability/       # Logfire setup, tracing configuration
```

## Formatting

- Rust: `cargo fmt --all` (default rustfmt settings)
- Markdown, JSON, TOML: `dprint fmt`
- Run `just fmt` to format everything
- Line width: 88 characters for dprint-formatted files

## Security

- Never commit API keys or tokens.
- Use env vars / `.env` for secrets.
- Discord bot requires `discord.allowed_user_id` in config — rejects all other users.

## Specs

- `specs/TODO.md`: Ordered task list for reaching PoC. Each task references a spec file.
- `specs/*.md`: One spec per feature/task.
- `specs/backlog/*.md`: Future features not in the PoC path.
