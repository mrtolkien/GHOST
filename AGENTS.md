# GHOST - AI assistant agents guidelines

Baseline for all agent runs. `AGENTS.md` is symlinked here. Feature specs in `specs/`.
Update this file when project-wide rules change or info becomes stale.

## HOW TO WORK

### Code Design

ASK QUESTIONS ABOUT DESIGN. DON'T JUST START WRITING CODE.

You are _great_ at writing code, but _horrendous_ at designing systems and products.
Make extremely few assumptions. Regularly ask if your approach and understanding are
right. DO NOT MAKE ASSUMPTIONS ABOUT WHAT THE USER WANTS: ASK THEM.

### Temporary Fixes Require a Second Pass

When a plan tells you to make a temporary change to keep things compiling (stub values,
placeholder arguments, `None`/`todo!()`) — those are not done. Before considering the
feature complete, revisit every temporary change and decide whether it needs a real
implementation. The plan only covers the paths its author thought of; the ones they
forgot become production bugs.

### Maintainability Over Expedience

You have a strong bias toward short-term fixes. You will instinctively reach for the
quick patch — hard-coded values, special-case branches, copy-pasted blocks, brittle
workarounds that pass the immediate test but make the codebase worse. **Resist this.**
When fixing a bug or adding a feature, find the _right_ solution: the one that makes the
next change easier, not harder. If the correct fix requires refactoring surrounding
code, moving responsibilities, or rethinking an abstraction — do that work. A clean fix
that takes longer is always preferable to a dirty fix that ships faster. Never introduce
a hack "for now" without explicitly flagging it and getting user approval. Causing
regressions in existing behavior through careless, rushed changes is worse than slower
delivery.

### Use Existing Abstractions — Never Reimplement

Before writing any code that talks to an external service (LLM provider, Discord, HTTP
endpoint, database), **search for an existing abstraction that already does it**. The
codebase has traits, factories, and typed enums for a reason. If you need to call an LLM
provider, use the `Provider` trait and `create_provider()`. If you need a config enum,
check if one exists before inventing a parallel one.

**Concrete rules:**

- Never hand-roll HTTP requests to a service that already has a provider/client
  implementation. You WILL get the URL, headers, auth, or request format wrong.
- Never create a parallel enum that mirrors an existing one. If `config::ProviderKind`
  exists, use it — don't create `onboarding::ProviderChoice`.
- Never convert a typed enum to a string for matching. If you find yourself writing
  `match thing.as_str() { "foo" => ... }`, the match should be on the enum directly.
- When adding a new module that needs to interact with existing subsystems, read the
  existing traits and factories first. The 10 minutes you spend reading saves hours of
  debugging divergent reimplementations.

### Typed Enums Over String Matching

Use enums for anything that has a fixed set of variants. Match on the enum, not on
stringified versions of it. Strings are for serialization boundaries (TOML, JSON, CLI
flags) — once parsed, everything should be typed. If you see `match s { "foo" => ... }`,
ask whether an enum exists or should be created.

## Project Overview

GHOST is a personal AI agent platform. A single binary (`ghost`) runs one GHOST for one
OPERATOR — persistent memory, background agents, multi-interface communication (Discord
primary for PoC).

**State**: We are currently in pre-alpha. The software is moving extremely fast. You are
allowed to make breaking changes requiring re-creating the full GHOST workspace if
implementing migrations would be too complicated, but communicate it clearly to the
user.

### Architecture

- Single binary, single crate, CLI-first
- One GHOST, one OPERATOR — identity in workspace files, not DB tables
- SQLite (sqlx) + sqlite-vec (KNN) + FTS5 — OpenTelemetry for observability
- Skills over tools — prefer agentskills.io skills + file reads over new tool APIs

### Core Concepts

- **GHOST/OPERATOR**: AI agent / human user. One each per installation.
- **Session**: Chat thread. **Agent**: Lua-defined autonomous worker (`agents/<name>/`).
- **Knowledge**: Notes/references/diary — dual storage: plain text files on disk (source
  of truth) + SQLite for FTS5/BM25 search + embeddings. References live under
  `references/{topic}/` in the workspace.
- **Skill**: agentskills.io files in `$WORKSPACE/skills/`, read via file tools.
- **Provider**: LLM backend (OpenRouter, Kimi, OpenAI OAuth/Codex Responses API).

## Design Philosophy

- **Text-first**: Plain text files as primary feature surface (agents, skills, identity,
  notes). Add a tool only when text + CLI can't deliver safely. Ask user first.
- **Prompt design**: System prompt stays generic; specific workflows live in skills;
  complex workflows get dedicated agents. See `specs/notes/prompt-design.md`.
- **MCPs**: Use `context7`, `rust-analyzer-mcp`, `gh` extensively. Prefer MCP-backed
  answers over assumptions.

## Code Quality Rules

### Script-First Execution (NON-NEGOTIABLE)

NEVER use `python3 -c`, `| jq`, `| awk`, or bash commands over ~80 chars for data
processing. Instead:

1. **Search first**: `ls scripts/` — find an existing script to extend or reuse
2. Write a Python script to `scripts/<topic>/` with uv inline metadata (PEP 723)
3. Run with `uv run scripts/<topic>/<name>.py [args]`
4. Throwaway scripts go in `scripts/tmp/` (gitignored)

Read the `/uv-scripts` skill before writing any script. A hook enforces this rule.

### Observability (NON-NEGOTIABLE)

Instrument meaningful execution boundaries. Full conventions in `/tracing` skill — read
before adding or modifying any instrumentation.

### Error Handling

- `thiserror` for all error types — domain-specific enums, not stringly-typed variants
- No `.unwrap()`/`.expect()` in production (tests fine). Propagate with `?` + context.
- One local error enum per module once it has behavior; convert up with `#[from]`
- Log at handling boundary, not creation. Every variant must be self-diagnosable.

### Code Structure

- Single crate. Max 4 levels of indentation — extract functions.
- Short `///` docs on non-obvious public items (what + why, not signature restating).
- Max ~500 LoC per file (excl. tests). No logic in `mod.rs` (barrel files only).
- Domain-named files (`config.rs`), not generic names (`service.rs`, `utils.rs`).

### Rust Style

- `just ci` for format + check + clippy + tests. Fix all issues before returning.
- `&str` over `String` when ownership not needed. `impl Trait` in arguments.
- `Arc` not `Rc`. `Debug` on all public types. `#[must_use]` where appropriate.

### Dependencies (do not change without discussion)

Tokio, Axum, reqwest (rustls), SQLite (sqlx) + sqlite-vec + FTS5, serenity,
opentelemetry + tracing-opentelemetry, thiserror, serde, toml, chrono, Ollama (HTTP
API).

## Development Flow

- Small, atomic commits: `feat:`, `fix:`, `refactor:`, `test:`, `docs:`, `chore:`
- Run `just ci` at each step. Offer PR via `gh` MCP when on a branch.
- Integration tests with `live-tests` flag; unit tests only for complex logic. Read
  `/testing` skill before writing any test.

## Configuration

- Config: `~/.config/ghost/config.toml` (override: `GHOST_CONFIG_DIR` env var)
- Secrets: `.env` / env vars. Never commit API keys.
- Workspace: `~/GHOST/` default, configurable. GHOST modifies config via CLI commands.
- Discord: `discord.allowed_user_id` required — rejects all other users.

## Project Layout

```
src/
├── main.rs              # CLI entry point (clap)
├── cli/                 # CLI subcommands (thin — parse args, delegate)
├── daemon/              # Subsystem wiring, task spawning, graceful shutdown
├── config.rs            # Config types, loading, defaults
├── config_cli.rs        # CLI config get/set operations
├── config_workspace.rs  # Workspace bootstrapping
├── db/                  # SQLite schema (sqlx migrations), queries, connection
│   └── knowledge/       # Notes, references, diary (crud, search, graph, stats)
├── providers/           # Provider trait + implementations
├── chat/                # Chat orchestration, session management, compaction
│   └── tool_loop.rs     # Shared tool-use loop (ToolLoopHandler trait)
├── tools/               # Tool definitions and implementations
├── agents/              # Lua agent loading, scheduling, runner, watcher
│   └── scheduler.rs     # Unified cron + idle scheduler
├── scripting/           # Lua VM (ScriptHost), nudge library, custom tools
├── reflection.rs        # Reflection utilities (web cache curation, cited edges)
├── knowledge/           # Knowledge types, wiki links, file operations
├── interfaces/discord/  # Discord bot transport, DiscordSender
├── prompt/              # System prompt rendering
├── web/                 # Web search, web fetch, web cache
├── auth/                # Authentication helpers
├── embeddings/          # Embedding pipeline (Ollama)
└── observability.rs     # OpenTelemetry setup, tracing configuration
```

## Formatting

- `just fmt` — line width 88 (oxfmt for md/JSON/TOML, cargo fmt for Rust)
- Docs content (`docs/src/content/**`) excluded from oxfmt (Starlight `:::` syntax)
- Prose: `GHOST` and `OPERATOR` in all caps. Code identifiers unchanged.

## Documentation & Specs

- User-facing docs: `docs/` (Astro Starlight). Read `/docs` skill before changes.

The following are relevant to the superpowers skills (brainstorm, writing plan, ...):

- Human-written specs: `backlog/tasks/{milestone}/*.md`
- Design plans: Append to the associated task. Do not create a new file for a design
  plan: add a markdown separator then write it there.
- Implementation plans: `backlog/plans`
- Finished specs, designs, and implementation plans: `backlog/completed`

## Similar projects

When asked to implement new features, analyze how similar projects do it:

- ZeroClaw, great Rust implementation with tons of providers, interfaces, and clean
  traits: https://github.com/zeroclaw-labs/zeroclaw
- OpenClaw, the OG: https://github.com/openclaw/openclaw
