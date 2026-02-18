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

## CRUCIAL

Always update this file when:

- Project-wide core rules change
- Information here is stale
- An important assumption documented here is proven wrong

## Project Overview

GHOST is a personal AI agent platform. A single binary (`ghost`) runs one GHOST for one
OPERATOR. It provides persistent memory, background jobs, and multi-interface
communication (Discord as the primary interface for the PoC).

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
- Skills over tools — prefer agentskills.io skills + file reads over adding new tool
  APIs

## Core Concepts

- **GHOST**: The AI agent. Identity defined by workspace files (BOOT.md, SOUL.md,
  OPERATOR.md). One per installation.
- **OPERATOR**: The human user. Identified by Discord user ID in config. One per
  installation.
- **Session**: Chat thread between OPERATOR and GHOST.
- **Job**: Markdown file in `$WORKSPACE/jobs/` with TOML frontmatter. Cron-scheduled.
  Heartbeat and reflection are dedicated subsystems (not regular jobs).
- **Knowledge**: Notes, references, and diary entries stored in SurrealDB with typed
  graph edges and embeddings search.
- **Skill**: agentskills.io-compatible files in `$WORKSPACE/skills/`. Read via standard
  file tools.
- **Provider**: LLM backend. Provider trait with implementations for OpenRouter, Kimi,
  and OpenAI OAuth (Codex Responses API).

## Workspace and Flow

- Prefer small, atomic commits with conventional commit messages (`feat:`, `fix:`,
  `refactor:`, `test:`, `docs:`, `chore:`).

## Text-First Feature Design

- Prefer plain text files in the GHOST workspace as the primary feature surface (jobs,
  skills, identity files, knowledge notes).
- Prefer skills + CLI workflows over adding new tool APIs.
- Add a dedicated tool only when text + existing tools + CLI cannot deliver the feature
  safely or ergonomically. Always ask the user before adding a tool.

## MCPs

Make extensive use of MCPs available to you:

- `context7` for up-to-date library documentation
- `rust-analyzer-mcp` for refactors and code actions
- `gh` for interacting with GitHub

Prefer MCP-backed answers over assumptions for library/framework behavior.

## Code Quality Rules

### Observability (NON-NEGOTIABLE)

Instrument meaningful execution boundaries with `#[instrument]` or `logfire::span!()`,
not every function. Prioritize operations where tracing materially helps debugging:
database calls, external service calls (provider APIs, Discord, embeddings), and run
entry points (for example, a received message/job tick handler). Keep small pure helpers
uninstrumented.

- Use `#[tracing::instrument(skip_all, fields(relevant_field = %value))]` on async
  functions
- Use `logfire::info!()`, `logfire::warn!()`, `logfire::error!()` for structured events
- Include relevant context in span fields (session ID, job name, provider, model, etc.)
- Log all external calls (provider API, Discord, embeddings) with timing
- Log all errors with full context before propagating
- Use `tracing::Span::current().record()` to add fields mid-execution when useful

### Error Handling

- Use `thiserror` for all error types — define domain-specific error enums
- Do not use stringly-typed catch-all variants like `Config(String)` or
  `Database(String)` for domain errors
- Each module should define at least one local error enum once it has meaningful
  behavior; convert into higher-level errors with `#[from]`
- No `.unwrap()` or `.expect()` in production code (tests are fine)
- Propagate errors with `?` — add context with `.map_err()` or a wrapper type
- Log errors at the boundary where they are handled, not where they are created
- Every error variant should carry enough context to diagnose the issue from logs alone

### Code Structure

- Single crate, organized with modules.
- If you need over 4 levels of indentation, break it into functions.
- Avoid excess comments: code should be expressive and readable. If it requires
  comments, it likely needs a refactor.
- Break down complex systems into clear functions or traits, and if required, multiple
  files with clear names.
- A file over 500 LoC (excluding tests) likely means a design issue. Humans search code
  through filenames.
- Do not put logic in `mod.rs` files — they should be mostly barrel files (re-exports).
- Prefer direct, domain-named module files when a module has one primary implementation:
  use `src/config.rs`, not `src/config/service.rs` or similarly generic names.
- Avoid generic filenames like `service.rs`, `manager.rs`, `utils.rs` unless scoped by a
  domain folder containing multiple concrete modules.

### Rust Style

- Use `just ci` to run format, check, clippy, and tests all at once. No need to run them
  one by one.
- Always fix all the issues in `just ci` before returning
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
- **Serialization**: serde + format-specific crates
- **Config**: toml
- **Time**: chrono
- **Embeddings**: Ollama (HTTP API)

## Development Flow

Iterate until all spec items are built and tested:

1. At each step, run `just ci`
2. Once an atomic feature is complete, make a conventional commit.
3. Offer the user to create a pull request with the `gh` MCP if working on a branch.

## Testing Strategy

- **Integration tests** with a `live-tests` feature flag for all major features.
- Maintain a robust, reusable integration test "starting state" (test fixtures) that can
  be shared across tests.
  - For example, adding a new provider should have integration tests validating that it
    accepts all our tools and is able to use at least one of them.
- **Unit tests** only when there are genuinely complex behaviors or external crate
  behaviors to validate.
- Live tests (`--features live-tests`) are human-run only by default. If you need to run
  them, ask the user.
- Test harness reference (all helpers, mock types, `LiveTestEnv` API):
  `specs/notes/test-harness.md`

### Test Readability (NON-NEGOTIABLE)

Tests must be **readable first**. A reader should understand what a test does in
seconds, not minutes. This means:

- Extract ALL setup boilerplate into helper functions. A test body should be setup +
  action + assert, each in 1-3 lines.
- Prefer `let workspace = test_workspace()` over 15 lines of `TempDir` + `fs::write` +
  `env::set_var` inline.
- Build up a `tests/common/` module of reusable helpers from the start (spec 02). The
  `TestFixture` in spec 18 is the culmination, but helpers should exist before that.
- Name helpers after what they produce, not what they do: `test_config()` not
  `setup_config_dir_and_write_toml_and_set_env()`.
- Never duplicate setup across tests — if two tests need similar state, extract a
  helper.
- Extend existing helpers with parameters rather than creating new ones. If
  `test_config()` works for most tests but one needs a custom model, add an optional
  parameter — don't write `test_config_with_custom_model()` or inline the setup.
- This is critical because LLMs are biased toward generating inline boilerplate. Fight
  that instinct. Every test should read like a specification.

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
├── cli/                 # CLI subcommands (thin — parse args, delegate)
├── daemon/              # Subsystem wiring, task spawning, graceful shutdown
├── config.rs            # Config types, loading, defaults
├── config_cli.rs        # CLI config get/set operations
├── config_workspace.rs  # Workspace bootstrapping
├── db/                  # SurrealDB schema, queries, connection
│   ├── knowledge/       # Notes, references, diary (crud, search, graph, stats)
│   └── query.rs         # Shared query helpers (take_one, take_many, query_exec)
├── providers/           # Provider trait + implementations (OpenRouter, Kimi, OpenAI OAuth)
├── chat/                # Chat orchestration, session management, compaction
│   └── tool_loop.rs     # Shared tool-use loop (ToolLoopHandler trait)
├── tools/               # Tool definitions and implementations
├── jobs/                # Cron scheduling, heartbeat, reflection
├── knowledge/           # Knowledge types, wiki links, file operations
├── interfaces/discord/  # Discord bot transport, DiscordSender
├── prompt/              # System prompt rendering
├── web/                 # Web search, web fetch, web cache
├── auth/                # Authentication helpers
├── embeddings/          # Embedding pipeline (Ollama)
└── observability.rs     # Logfire setup, tracing configuration
```

## Formatting

- Run `just fmt` to format everything
- Line width: 88 characters for dprint-formatted files
- Terminology rule for docs/prose: always write `GHOST` and `OPERATOR` in all caps. Keep
  real code/file identifiers (crate names, paths, variable names) unchanged.

## Security

- Never commit API keys or tokens.
- Use env vars / `.env` for secrets.
- Discord bot requires `discord.allowed_user_id` in config — rejects all other users.

## Specs

- `specs/TODO.md`: Ordered task list for reaching PoC. Each task references a spec file.
- `specs/*.md`: One spec per feature/task.
- `specs/backlog/*.md`: Future features not in the PoC path.
