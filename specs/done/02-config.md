# 02 — Configuration System

## Overview

Configuration is split into two layers:

1. **config.toml** — Non-sensitive settings (models, workspace path, Discord config,
   tool settings, timing)
2. **.env / environment variables** — Secrets (API keys, tokens)

Config lives at `~/.config/ghost/config.toml` by default. Override with
`GHOST_CONFIG_DIR` env var.

## Config Structure

```toml
# ~/.config/ghost/config.toml

# Workspace directory (default: ~/GHOST)
workspace = "~/GHOST"

[models]
default = "primary"

[models.primary]
provider = "openrouter"
model = "anthropic/claude-sonnet-4-5-20250929"
# context_window = 200000  # optional override
# headers = {}             # optional extra headers

[discord]
enabled = true
allowed_user_id = "123456789"

[web]
search_provider = "brave"
search_max_results = 5

[embeddings]
url = "http://127.0.0.1:11434"
model = "qwen3-embedding:8b"
batch_size = 32

[timing]
heartbeat_idle_minutes = 5
heartbeat_check_seconds = 60
heartbeat_continue_minutes = 30
reflection_idle_minutes = 15

[compaction]
threshold = 0.85
keep_window = 20
```

## Environment Variables

```bash
OPENROUTER_API_KEY=...
DISCORD_BOT_TOKEN=...
BRAVE_API_KEY=...
KIMI_API_KEY=...           # For Kimi Code provider (05a)
# Future:
# ANTHROPIC_API_KEY=...
# GEMINI_API_KEY=...
```

## Rust Types

Two-layer approach: `Settings` (raw TOML with `Option<T>`) and `Config` (resolved with
concrete defaults).

```rust
/// Raw TOML deserialization target. All fields optional for partial configs.
#[derive(Debug, Deserialize)]
pub struct Settings {
    pub workspace: Option<String>,
    pub models: Option<ModelsSettings>,
    pub discord: Option<DiscordSettings>,
    pub web: Option<WebSettings>,
    pub embeddings: Option<EmbeddingsSettings>,
    pub timing: Option<TimingSettings>,
    pub compaction: Option<CompactionSettings>,
}

/// Resolved config with defaults applied. This is what the app uses.
#[derive(Debug, Clone)]
pub struct Config {
    pub workspace: PathBuf,
    pub models: ModelsConfig,
    pub discord: DiscordConfig,
    pub web: WebConfig,
    pub embeddings: EmbeddingsConfig,
    pub timing: TimingConfig,
    pub compaction: CompactionConfig,
}
```

## CLI Config Commands

`ghost config get <key>` — Print a config value (dot-separated path, e.g.,
`models.primary.model`)

`ghost config set <key> <value>` — Modify config.toml. This is how the GHOST can safely
change its own configuration without hand-editing TOML.

## Workspace Bootstrap

On first run (or `ghost init`), create the workspace directory structure:

```
~/GHOST/
├── BOOT.md            # Core information (template provided)
├── SOUL.md            # Evolving self-model (starts empty)
├── OPERATOR.md        # Operator knowledge (starts empty)
├── jobs/              # Cron job definitions
├── skills/            # agentskills.io skill files
├── .web-cache/        # Transient web results
└── knowledge/         # (managed by SurrealDB, but reference files live here)
```

## Testing with Temp Workspaces

This is the first feature that touches the filesystem (workspace bootstrap, config
read/write). All tests that need a workspace MUST use a temporary directory — never the
real `~/.config/ghost/` or `~/GHOST/`.

Start building a `tests/common/` module here with reusable helpers. Tests should read
like specifications, not setup scripts:

```rust
// tests/common/mod.rs — reusable helpers, built up over time

/// Returns a (Config, TempDir, TempDir) with temp workspace and config dir.
/// Config dir has GHOST_CONFIG_DIR set. TempDirs are kept alive by the caller.
pub fn test_config() -> (Config, TempDir, TempDir) { ... }

/// Returns a (Config, TempDir, TempDir) with a bootstrapped workspace
/// (BOOT.md, SOUL.md, OPERATOR.md, jobs/, skills/, etc. already created).
pub fn test_workspace() -> (Config, TempDir, TempDir) { ... }
```

Tests that use these helpers stay readable:

```rust
#[test]
fn config_loads_defaults_for_missing_fields() {
    let (config, _workspace, _config_dir) = test_config();
    assert_eq!(config.compaction.threshold, 0.85);
}

#[test]
fn workspace_bootstrap_creates_identity_files() {
    let (config, workspace, _config_dir) = test_workspace();
    assert!(workspace.path().join("BOOT.md").exists());
    assert!(workspace.path().join("SOUL.md").exists());
}
```

Add `tempfile` as a dev-dependency in this step:

```toml
[dev-dependencies]
tempfile = "3"
```

This `tests/common/` module grows with each spec. The `TestFixture` in spec 18 is the
full-featured version, but helpers should exist from the start.

## Validation

1. `cargo run -- init` — creates workspace at `~/GHOST/` with identity files (BOOT.md,
   SOUL.md, OPERATOR.md) and directories (jobs/, skills/)
2. Create a `~/.config/ghost/config.toml` with a custom workspace path — verify it's
   respected
3. `GHOST_CONFIG_DIR=/tmp/ghost-test cargo run -- init` — uses the override path
4. `cargo test` — config loading tests and temp workspace helper tests pass
5. `just ci` — passes

## Acceptance Criteria

- Config loads from `~/.config/ghost/config.toml` with defaults for missing fields
- `GHOST_CONFIG_DIR` env var overrides config path
- Secrets load from `.env` or environment
- `ghost config get models.default` prints the default model alias
- `ghost config set workspace /custom/path` modifies config.toml correctly
- Workspace directory is created on first run with template identity files
- Invalid config produces clear error messages with file path and field name
- Tests use temp directories — never touch the real `~/.config/ghost/` or `~/GHOST/`
- `just ci` passes
