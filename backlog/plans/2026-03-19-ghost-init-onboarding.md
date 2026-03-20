# `ghost init` Onboarding Wizard — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or superpowers:executing-plans
> to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the Python onboarding wizard with a native Rust `ghost init` that
walks users through LLM provider setup, Discord configuration, service installation
(nix + podman/docker), and first-run health checks — fully scriptable via CLI flags.

**Architecture:** `src/cli/init.rs` becomes a thin entry point that delegates to
`src/onboarding/` — a new module with submodules for detection, provider setup, Discord,
services, config writing, service file generation, health checks, and an on-demand AI
assistant. A bundled `services` skill teaches the GHOST how to manage its infrastructure
post-install.

**Tech Stack:** Rust, cliclack (wizard UX), dialoguer (FuzzySelect fallback), clap (CLI
flags), toml_edit (config diffing), reqwest (health probes + API validation).

**Spec:** `backlog/tasks/4-easy-install/5-onboarding.md` (design spec section)

**Testing skill:** Read `/testing` before writing any test. Read `/e2e-testing` for
stepwise/daemon tests. Read `/tracing` before adding instrumentation.

---

## File Structure

### New files

| File                                          | Responsibility                                                                                                                                      |
| --------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------- |
| `src/onboarding/mod.rs`                       | Barrel: re-exports, `OnboardingState` struct, `OnboardingError` type, choice enums                                                                  |
| `src/onboarding/wizard.rs`                    | Main wizard orchestration — phases 0-5, existing config handling, retry loops, `[h]` hotkey wiring                                                  |
| `src/onboarding/detect.rs`                    | `DetectedEnvironment` struct + `detect()` — probes nix, platform, container runtime, running services, existing config, nix packages, system memory |
| `src/onboarding/provider.rs`                  | Provider picker, API key/OAuth prompts, real validation call                                                                                        |
| `src/onboarding/discord.rs`                   | Discord setup guidance box, token + user ID prompts, token validation                                                                               |
| `src/onboarding/services.rs`                  | Per-service prompts (embeddings, search, crawl, docling), nix profile install, compose file generation                                              |
| `src/onboarding/config_writer.rs`             | Build `config.toml` + `.env` from wizard answers, diff display, write                                                                               |
| `src/onboarding/service_files.rs`             | systemd/launchd unit templates for daemon + native services, install + enable                                                                       |
| `src/onboarding/health.rs`                    | HTTP health probes per service, status table display                                                                                                |
| `src/onboarding/agent.rs`                     | On-demand onboarding AI assistant (`[h]` hotkey), mini chat in terminal                                                                             |
| `assets/skills/services/skill.md`             | Bundled skill: service management (main)                                                                                                            |
| `assets/skills/services/observability.md`     | Skill extra: SigNoz OTEL stack                                                                                                                      |
| `assets/skills/services/tailscale.md`         | Skill extra: Tailscale setup                                                                                                                        |
| `assets/onboarding-agent-prompt.md`           | System prompt for the onboarding assistant                                                                                                          |
| `assets/services/docker-compose.searxng.yml`  | SearXNG compose template fragment                                                                                                                   |
| `assets/services/docker-compose.crawl4ai.yml` | Crawl4AI + Chrome compose template fragment                                                                                                         |
| `assets/services/docker-compose.docling.yml`  | Docling compose template fragment (optional)                                                                                                        |
| `assets/services/searxng-settings.yml`        | SearXNG config (moved from `deploy/common/`)                                                                                                        |

### Modified files

| File                           | Changes                                                            |
| ------------------------------ | ------------------------------------------------------------------ |
| `src/cli/init.rs`              | Rewrite: parse new CLI flags (clap), delegate to `src/onboarding/` |
| `src/main.rs:17`               | Update `Init` variant to hold new `InitArgs` struct                |
| `src/lib.rs`                   | Add `pub mod onboarding;` declaration                              |
| `src/config_workspace.rs:8-36` | Add `services/` to `bootstrap_workspace_dirs`                      |
| `Cargo.toml`                   | Add `cliclack`, `dialoguer`, `sysinfo` dependencies                |

### Not modified (read-only references)

| File                                     | Used for                                                   |
| ---------------------------------------- | ---------------------------------------------------------- |
| `src/config.rs:74-87`                    | `Settings` struct shape — wizard must produce valid config |
| `src/config.rs:305-314`                  | `Provider` enum — must match wizard provider choices       |
| `src/config_cli.rs:25-60`                | `set_value_in_dir` — may reuse for config writing          |
| `src/providers/anthropic/credentials.rs` | OAuth credential path (`~/.claude/.credentials.json`)      |
| `src/auth/openai_oauth.rs`               | OpenAI OAuth device-code flow                              |
| `deploy/common/onboard.py`               | Reference for what config sections to write                |
| `deploy/common/searxng-settings.yml`     | Template for SearXNG config                                |

---

## Task 1: Add dependencies + module skeleton

**Files:**

- Modify: `Cargo.toml:6-88`
- Modify: `src/lib.rs:1-30`
- Create: `src/onboarding/mod.rs`

- [ ] **Step 1: Add crate dependencies to Cargo.toml**

Add to `[dependencies]`:

```toml
cliclack = "0.3"
dialoguer = { version = "0.11", features = ["fuzzy-select"] }
sysinfo = "0.35"
```

**NOTE: These require discussion per CLAUDE.md dependency rules.** All three are
onboarding-only (not used by the daemon runtime). `sysinfo` is for memory detection
(smart defaults on low-RAM systems). If `sysinfo` is rejected as too heavy, fall back to
reading `/proc/meminfo` on Linux and `sysctl hw.memsize` on macOS directly.

- [ ] **Step 2: Declare the onboarding module in lib.rs**

In `src/lib.rs`, add after the existing module declarations:

```rust
pub mod onboarding;
```

- [ ] **Step 3: Create the onboarding module barrel file**

Create `src/onboarding/mod.rs`:

```rust
pub mod detect;
pub mod wizard;
pub mod provider;
pub mod discord;
pub mod services;
pub mod config_writer;
pub mod service_files;
pub mod health;
pub mod agent;

/// Tracks cumulative wizard state across phases.
#[derive(Debug, Default)]
pub struct OnboardingState {
    pub provider: Option<ProviderChoice>,
    pub api_key: Option<String>,
    pub model: Option<String>,
    pub context_window: Option<u32>, // u32 — matches config.rs ModelSettings
    pub discord_token: Option<String>,
    pub discord_user_id: Option<String>,
    pub embeddings: Option<ServiceChoice>,
    pub embedding_model: Option<String>, // e.g. "qwen3-embedding:8b"
    pub search: Option<SearchChoice>,
    pub crawl: Option<ServiceChoice>,
    pub docling: Option<ServiceChoice>,
}

/// Must map to `config::Provider` enum string values.
/// NOTE: `OpenAiOAuth` (not "ChatGptOAuth") matches `Provider::as_str() -> "openai_oauth"`.
#[derive(Debug, Clone)]
pub enum ProviderChoice {
    OpenRouter,
    Anthropic,
    Kimi,
    OpenAiOAuth,
}

impl ProviderChoice {
    /// Parse from CLI flag value. Returns Err for unknown providers.
    pub fn from_flag(s: &str) -> Result<Self, OnboardingError> {
        match s {
            "openrouter" => Ok(Self::OpenRouter),
            "anthropic" => Ok(Self::Anthropic),
            "kimi" => Ok(Self::Kimi),
            "openai-oauth" | "chatgpt-oauth" | "openai_oauth" => Ok(Self::OpenAiOAuth),
            _ => Err(OnboardingError::InvalidInput(format!(
                "unknown provider: {s}"
            ))),
        }
    }

    /// Config-compatible string (matches `Provider::as_str()`).
    pub fn as_config_str(&self) -> &'static str {
        match self {
            Self::OpenRouter => "openrouter",
            Self::Anthropic => "anthropic",
            Self::Kimi => "kimi",
            Self::OpenAiOAuth => "openai_oauth",
        }
    }
}

#[derive(Debug, Clone)]
pub enum ServiceChoice {
    /// Install via nix profile and run as systemd/launchd service.
    NixNative,
    /// Run in the container stack (podman/docker compose).
    Container,
    /// Use an existing remote endpoint.
    Remote(String),
    /// Skip this service entirely.
    Skip,
}

impl ServiceChoice {
    pub fn from_flag(s: &str) -> Result<Self, OnboardingError> {
        match s {
            "local" | "nix" => Ok(Self::NixNative),
            "container" | "docker" | "podman" => Ok(Self::Container),
            "skip" => Ok(Self::Skip),
            s if s.starts_with("remote:") => Ok(Self::Remote(s[7..].to_string())),
            _ => Err(OnboardingError::InvalidInput(format!(
                "invalid service choice: {s}"
            ))),
        }
    }
}

#[derive(Debug, Clone)]
pub enum SearchChoice {
    SearxngLocal,
    BraveApi(String),
    SearxngRemote(String),
    Skip,
}

/// Module-local error type. Converts to `GhostError` via `#[from]`.
#[derive(Debug, thiserror::Error)]
pub enum OnboardingError {
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("provider validation failed: {0}")]
    ProviderValidation(String),
    #[error("discord validation failed: {0}")]
    DiscordValidation(String),
    #[error("nix install failed: {0}")]
    NixInstall(String),
    #[error("service health check failed: {0}")]
    HealthCheck(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Request(#[from] reqwest::Error),
}
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo check` Expected: compiles with no errors (module is declared but has only
types, no dependencies on other modules yet).

- [ ] **Step 5: Commit**

```
git add Cargo.toml src/lib.rs src/onboarding/mod.rs
git commit -m "feat: add onboarding module skeleton and dependencies"
```

---

## Task 2: Environment detection (`detect.rs`)

**Files:**

- Create: `src/onboarding/detect.rs`
- Modify: `src/onboarding/mod.rs` (add `pub mod detect;` — already in Task 1)

- [ ] **Step 1: Write test for detection logic**

Create unit tests at the bottom of `detect.rs`. These test the pure logic (parsing probe
results), not the actual system probes:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_platform_linux() {
        // cfg!(target_os = "linux") is compile-time, so we test the enum
        let platform = Platform::detect();
        // Just verify it doesn't panic and returns a valid variant
        assert!(matches!(
            platform,
            Platform::Linux | Platform::MacOs | Platform::Other(_)
        ));
    }

    #[test]
    fn container_runtime_prefers_podman() {
        // If both are "found", podman wins
        let rt = ContainerRuntime::from_which_results(true, true);
        assert!(matches!(rt, Some(ContainerRuntime::Podman)));
    }

    #[test]
    fn container_runtime_falls_back_to_docker() {
        let rt = ContainerRuntime::from_which_results(false, true);
        assert!(matches!(rt, Some(ContainerRuntime::Docker)));
    }

    #[test]
    fn container_runtime_none_if_neither() {
        let rt = ContainerRuntime::from_which_results(false, false);
        assert!(rt.is_none());
    }

    #[test]
    fn low_memory_detection() {
        assert!(is_low_memory(2 * 1024 * 1024 * 1024)); // 2GB
        assert!(!is_low_memory(8 * 1024 * 1024 * 1024)); // 8GB
    }
}
```

- [ ] **Step 2: Run test — verify it fails**

Run: `cargo test --lib onboarding::detect` Expected: FAIL — `detect` module doesn't have
the types/functions yet.

- [ ] **Step 3: Implement `detect.rs`**

```rust
use std::path::PathBuf;
use std::process::Command;

use sysinfo::System;

const LOW_MEMORY_THRESHOLD: u64 = 4 * 1024 * 1024 * 1024; // 4GB

/// All environment facts gathered before interactive prompts begin.
#[derive(Debug)]
pub struct DetectedEnvironment {
    pub nix_installed: bool,
    pub platform: Platform,
    pub container_runtime: Option<ContainerRuntime>,
    pub llama_server_in_path: bool,
    pub docling_serve_in_path: bool,
    pub services_running: RunningServices,
    pub existing_config: Option<PathBuf>,
    pub existing_env: Option<PathBuf>,
    pub low_memory: bool,
    pub total_memory_bytes: u64,
}

#[derive(Debug)]
pub enum Platform {
    Linux,
    MacOs,
    Other(String),
}

#[derive(Debug, Clone)]
pub enum ContainerRuntime {
    Podman,
    Docker,
}

#[derive(Debug, Default)]
pub struct RunningServices {
    pub llama_server: bool,   // :11434
    pub searxng: bool,        // :8080
    pub chrome: bool,         // :9222
    pub crawl4ai: bool,       // port TBD
    pub docling: bool,        // :5001
}

impl Platform {
    pub fn detect() -> Self {
        if cfg!(target_os = "linux") {
            Self::Linux
        } else if cfg!(target_os = "macos") {
            Self::MacOs
        } else {
            Self::Other(std::env::consts::OS.to_string())
        }
    }

    pub fn is_linux(&self) -> bool {
        matches!(self, Self::Linux)
    }

    pub fn is_macos(&self) -> bool {
        matches!(self, Self::MacOs)
    }
}

impl ContainerRuntime {
    pub fn from_which_results(podman_found: bool, docker_found: bool) -> Option<Self> {
        if podman_found {
            Some(Self::Podman)
        } else if docker_found {
            Some(Self::Docker)
        } else {
            None
        }
    }

    pub fn compose_command(&self) -> &'static str {
        match self {
            Self::Podman => "podman",
            Self::Docker => "docker",
        }
    }
}

pub fn is_low_memory(total_bytes: u64) -> bool {
    total_bytes < LOW_MEMORY_THRESHOLD
}

/// Run all detection checks. This is fast (~1s) and makes no interactive prompts.
pub async fn detect() -> DetectedEnvironment {
    let nix_installed = which_exists("nix");
    let platform = Platform::detect();
    let podman = which_exists("podman");
    let docker = which_exists("docker");
    let container_runtime = ContainerRuntime::from_which_results(podman, docker);
    let llama_server_in_path = which_exists("llama-server");
    let docling_serve_in_path = which_exists("docling-serve");

    let services_running = probe_running_services().await;

    let (existing_config, existing_env) = match crate::config::config_dir() {
        Ok(config_dir) => {
            let config_path = config_dir.join("config.toml");
            let env_path = config_dir.join(".env");
            (
                config_path.exists().then_some(config_path),
                env_path.exists().then_some(env_path),
            )
        }
        Err(_) => (None, None),
    };

    let mut sys = System::new();
    sys.refresh_memory();
    let total_memory_bytes = sys.total_memory();

    DetectedEnvironment {
        nix_installed,
        platform,
        container_runtime,
        llama_server_in_path,
        docling_serve_in_path,
        services_running,
        existing_config,
        existing_env,
        low_memory: is_low_memory(total_memory_bytes),
        total_memory_bytes,
    }
}

fn which_exists(binary: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths).any(|dir| dir.join(binary).is_file())
        })
        .unwrap_or(false)
}

async fn probe_running_services() -> RunningServices {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()
        .unwrap_or_default();

    let (llama, searxng, chrome, crawl4ai, docling) = tokio::join!(
        probe_http(&client, "http://127.0.0.1:11434/health"),
        probe_http(&client, "http://127.0.0.1:8080"),
        probe_http(&client, "http://127.0.0.1:9222/json/version"),
        probe_http(&client, "http://127.0.0.1:11235/health"),
        probe_http(&client, "http://127.0.0.1:5001/health"),
    );

    RunningServices {
        llama_server: llama,
        searxng,
        chrome,
        crawl4ai,
        docling,
    }
}

async fn probe_http(client: &reqwest::Client, url: &str) -> bool {
    client.get(url).send().await.is_ok()
}
```

- [ ] **Step 4: Run tests — verify they pass**

Run: `cargo test --lib onboarding::detect` Expected: all 5 tests pass.

- [ ] **Step 5: Run `cargo check` for the full crate**

Run: `cargo check` Expected: compiles. `detect()` references
`crate::config::config_dir()` — verify this function exists (it does, in
`src/config.rs`). If the function name is different, adjust.

- [ ] **Step 6: Commit**

```
git add src/onboarding/detect.rs
git commit -m "feat: add environment detection for onboarding wizard"
```

---

## Task 3: CLI flags + wizard entry point (`init.rs` rewrite)

**Files:**

- Modify: `src/cli/init.rs` (full rewrite)
- Modify: `src/main.rs:17` (update `Init` variant)

- [ ] **Step 1: Define the `InitArgs` struct with clap flags**

Rewrite `src/cli/init.rs`:

```rust
use clap::Args;

use crate::onboarding::detect;

#[derive(Args, Debug)]
pub struct InitArgs {
    /// LLM provider: openrouter, anthropic, kimi, chatgpt-oauth
    #[arg(long)]
    pub provider: Option<String>,

    /// API key for the selected provider
    #[arg(long)]
    pub api_key: Option<String>,

    /// Model ID (e.g. "anthropic/claude-sonnet-4")
    #[arg(long)]
    pub model: Option<String>,

    /// Context window size in tokens
    #[arg(long)]
    pub context_window: Option<u32>,

    /// Discord bot token
    #[arg(long)]
    pub discord_token: Option<String>,

    /// Discord user ID (numeric)
    #[arg(long)]
    pub discord_user: Option<String>,

    /// Embeddings setup: local, remote:<url>, skip
    #[arg(long)]
    pub embeddings: Option<String>,

    /// Web search setup: local, brave:<key>, remote:<url>, skip
    #[arg(long)]
    pub search: Option<String>,

    /// Web fetch setup: local, remote:<url>, skip
    #[arg(long)]
    pub crawl: Option<String>,

    /// Document processing: local, container, remote:<url>, skip
    #[arg(long)]
    pub docling: Option<String>,

    /// Start all services after setup
    #[arg(long)]
    pub start: bool,
}

pub async fn execute(args: InitArgs) -> Result<(), crate::GhostError> {
    // Phase 0: Detection
    let env = detect::detect().await;

    if !env.nix_installed {
        cliclack::outro_cancel(
            "Nix is required but not installed.\n\
             Install it from: https://install.determinate.systems/nix"
        )?;
        return Err(crate::GhostError::Config(
            "Nix is not installed".into()
        ));
    }

    // Display detection results
    display_detection_results(&env);

    // TODO: Phases 1-5 will be added in subsequent tasks
    // Each phase module returns its portion of OnboardingState

    Ok(())
}

fn display_detection_results(env: &detect::DetectedEnvironment) {
    cliclack::intro("GHOST — First-time setup").expect("intro");

    let checks = [
        (true, "Nix installed"),
        (true, &format!("Platform: {:?}", env.platform)),
        (
            env.container_runtime.is_some(),
            &format!(
                "Container runtime: {}",
                env.container_runtime
                    .as_ref()
                    .map_or("not found", |r| match r {
                        detect::ContainerRuntime::Podman => "Podman",
                        detect::ContainerRuntime::Docker => "Docker",
                    })
            ),
        ),
    ];

    for (ok, label) in &checks {
        if *ok {
            cliclack::log::success(label).expect("log");
        } else {
            cliclack::log::warning(label).expect("log");
        }
    }
}
```

- [ ] **Step 2: Update `Commands::Init` in `src/main.rs` to pass `InitArgs`**

In `src/main.rs`, change the `Init` variant from a unit variant to:

```rust
Init(ghost::cli::init::InitArgs),
```

And in `dispatch()`, change the arm to:

```rust
Commands::Init(args) => ghost::cli::init::execute(args).await,
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check` Expected: compiles. The wizard is a skeleton that only runs Phase 0
detection.

- [ ] **Step 4: Manual smoke test**

Run: `cargo run -- init --help` Expected: shows all the new `--provider`, `--api-key`,
etc. flags.

- [ ] **Step 5: Commit**

```
git add src/cli/init.rs src/main.rs
git commit -m "feat: rewrite ghost init with CLI flags and detection phase"
```

---

## Task 4: Provider setup (`provider.rs`)

**Files:**

- Create: `src/onboarding/provider.rs`
- Modify: `src/onboarding/mod.rs` (add `pub mod provider;`)

- [ ] **Step 1: Write test for provider validation call**

At the bottom of `provider.rs`, add a test that the validation function returns an error
for an invalid API key (this can be a unit test with a mock or a simple format-check
test):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_provider_choice_from_flag() {
        assert!(matches!(
            ProviderChoice::from_flag("openrouter"),
            Ok(ProviderChoice::OpenRouter)
        ));
        assert!(matches!(
            ProviderChoice::from_flag("anthropic"),
            Ok(ProviderChoice::Anthropic)
        ));
        // All aliases for OpenAI OAuth
        assert!(matches!(
            ProviderChoice::from_flag("openai-oauth"),
            Ok(ProviderChoice::OpenAiOAuth)
        ));
        assert!(matches!(
            ProviderChoice::from_flag("chatgpt-oauth"),
            Ok(ProviderChoice::OpenAiOAuth)
        ));
        assert!(ProviderChoice::from_flag("invalid").is_err());
    }

    #[test]
    fn provider_config_string_matches_config_rs() {
        // These must match Provider::as_str() in config.rs
        assert_eq!(ProviderChoice::OpenRouter.as_config_str(), "openrouter");
        assert_eq!(ProviderChoice::Kimi.as_config_str(), "kimi");
        assert_eq!(ProviderChoice::OpenAiOAuth.as_config_str(), "openai_oauth");
        assert_eq!(ProviderChoice::Anthropic.as_config_str(), "anthropic");
    }

    #[test]
    fn catalog_url_per_provider() {
        assert!(catalog_url(&ProviderChoice::OpenRouter).contains("openrouter.ai"));
        assert!(catalog_url(&ProviderChoice::Kimi).contains("kimi.com"));
    }
}
```

- [ ] **Step 2: Run test — verify it fails**

Run: `cargo test --lib onboarding::provider` Expected: FAIL

- [ ] **Step 3: Implement `provider.rs`**

Core functions (all `from_flag` methods are already on `ProviderChoice` in `mod.rs`):

- `catalog_url(provider: &ProviderChoice) -> &'static str` — return provider's model
  catalog URL
- `prompt_provider(flag: Option<&str>) -> Result<ProviderChoice>` — interactive picker
  or parse flag
- `prompt_credentials(provider: &ProviderChoice, flag: Option<&str>) -> Result<Option<String>>`
  — for API key providers (OpenRouter, Kimi): password prompt or flag. For Anthropic:
  check `~/.claude/.credentials.json` exists, read `claudeAiOauth` section (reuse
  `crate::providers::anthropic::credentials::load_credentials()`). If file missing →
  show note "Run `claude` first to authenticate" and return error. For OpenAiOAuth:
  check for existing tokens, if missing → run device-code OAuth flow inline (call
  `crate::auth::openai_oauth::run_codex_auth_flow()` or equivalent). On headless
  servers: device-code flow prints URL + code for remote auth.
- `prompt_model(provider: &ProviderChoice, flag: Option<&str>) -> Result<String>` — show
  catalog URL in a `cliclack::note()` box, text input
- `prompt_context_window(flag: Option<u32>) -> Result<u32>` — text input with default
- `validate_provider(provider: &ProviderChoice, api_key: Option<&str>, model: &str) -> Result<()>`
  — real completion request via reqwest. For OAuth providers, uses loaded credentials.
  On failure: show error, offer retry (up to 3 attempts) or go back to provider select.

The validation call is a minimal chat completion:

```json
{
  "model": "<model_id>",
  "messages": [
    { "role": "system", "content": "Reply with OK" },
    { "role": "user", "content": "ping" }
  ],
  "max_tokens": 5
}
```

Sent to the provider's API endpoint. For OAuth providers, load credentials from the
appropriate path first.

- [ ] **Step 4: Run tests — verify they pass**

Run: `cargo test --lib onboarding::provider` Expected: PASS

- [ ] **Step 5: Wire into `execute()` in `init.rs`**

After Phase 0 detection, call the provider functions:

```rust
// Phase 1: Provider
let provider = provider::prompt_provider(args.provider.as_deref())?;
let api_key = provider::prompt_api_key(&provider, args.api_key.as_deref()).await?;
let model = provider::prompt_model(&provider, args.model.as_deref())?;
let context_window = provider::prompt_context_window(args.context_window)?;
provider::validate_provider(&provider, api_key.as_deref(), &model).await?;

cliclack::log::success("Provider verified — model responded successfully")?;
cliclack::log::info("Press [h] at any prompt for AI-assisted help")?;
```

- [ ] **Step 6: Commit**

```
git add src/onboarding/provider.rs src/onboarding/mod.rs src/cli/init.rs
git commit -m "feat: add provider selection and validation to onboarding"
```

---

## Task 5: Discord setup (`discord.rs`)

**Files:**

- Create: `src/onboarding/discord.rs`
- Modify: `src/onboarding/mod.rs` (add `pub mod discord;`)

- [ ] **Step 1: Write test for user ID validation**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_discord_user_id() {
        assert!(validate_user_id("123456789012345678").is_ok());
        assert!(validate_user_id("12345678901234567").is_ok()); // 17 digits
    }

    #[test]
    fn invalid_discord_user_id() {
        assert!(validate_user_id("abc").is_err());
        assert!(validate_user_id("123").is_err()); // too short
        assert!(validate_user_id("").is_err());
    }
}
```

- [ ] **Step 2: Implement `discord.rs`**

Core functions:

- `validate_user_id(id: &str) -> Result<()>` — numeric, 17-18 digits
- `validate_bot_token(token: &str) -> Result<()>` — real Discord API call: GET
  `https://discord.com/api/v10/users/@me` with `Authorization: Bot <token>`
- `prompt_discord(token_flag: Option<&str>, user_flag: Option<&str>) -> Result<(String, String)>`
  — shows the beautiful setup guide box via `cliclack::note()`, then prompts for token
  (password input) and user ID (text input)

The `cliclack::note()` call renders the Discord setup guide (steps 1-5 from the spec) in
a styled box.

- [ ] **Step 3: Run tests — verify they pass**

Run: `cargo test --lib onboarding::discord`

- [ ] **Step 4: Wire into `execute()` in `init.rs`**

After Phase 1, add Phase 2:

```rust
// Phase 2: Discord
let (discord_token, discord_user_id) = discord::prompt_discord(
    args.discord_token.as_deref(),
    args.discord_user.as_deref(),
).await?;
```

- [ ] **Step 5: Commit**

```
git add src/onboarding/discord.rs src/onboarding/mod.rs src/cli/init.rs
git commit -m "feat: add Discord setup to onboarding wizard"
```

---

## Task 6: Compose file templates + SearXNG config

**Files:**

- Create: `assets/services/docker-compose.searxng.yml`
- Create: `assets/services/docker-compose.crawl4ai.yml`
- Create: `assets/services/docker-compose.docling.yml`
- Create: `assets/services/searxng-settings.yml`

These must exist before Task 7 (`services.rs`) because the code uses `include_str!` to
embed them at compile time.

- [ ] **Step 1: Create SearXNG settings**

Copy from `deploy/common/searxng-settings.yml` to
`assets/services/searxng-settings.yml`.

- [ ] **Step 2: Create compose template fragments**

Each fragment defines one service block at 2-space indent (to be assembled under a
`services:` key). Include platform-conditional comments for port bindings.

See Task 11 (old numbering) for the exact YAML content of each fragment.

**IMPORTANT**: These compose templates in `assets/services/` are used as `include_str!`
source material for `services.rs` to generate the final compose file. They must NOT be
auto-installed to the workspace by the bundled files mechanism (`src/bundled.rs`). Check
`build.rs` to ensure `assets/services/` is either excluded from the bundle walk or the
install logic skips these paths. The wizard writes the composed result to
`<workspace>/services/docker-compose.yml` — the fragments themselves are never written.

- [ ] **Step 3: Verify bundling exclusion**

Check `build.rs` — if `assets/services/` would be bundled, add an exclusion. The compose
fragments are compile-time resources only.

- [ ] **Step 4: Commit**

```
git add assets/services/
git commit -m "feat: add compose templates and SearXNG config to assets"
```

---

## Task 7: Service setup (`services.rs`)

**Files:**

- Create: `src/onboarding/services.rs`
- Modify: `src/onboarding/mod.rs` (already has `pub mod services;` from Task 1)

This is the largest task. It handles per-service prompts, nix profile installation, and
compose file generation. Note: `ServiceChoice::from_flag` and `SearchChoice` are defined
on the types in `mod.rs` — this module uses them, doesn't redefine them.

- [ ] **Step 1: Write test for compose file generation**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compose_with_all_services() {
        let selections = ServiceSelections {
            searxng: true,
            crawl4ai: true,
            docling_container: false,
        };
        let compose = generate_compose(&selections, true); // linux = host networking
        assert!(compose.contains("searxng"));
        assert!(compose.contains("crawl4ai"));
        assert!(compose.contains("chrome"));
        assert!(!compose.contains("docling"));
    }

    #[test]
    fn compose_with_no_services() {
        let selections = ServiceSelections {
            searxng: false,
            crawl4ai: false,
            docling_container: false,
        };
        let compose = generate_compose(&selections, true);
        // Should still be valid YAML but with empty services
        assert!(compose.contains("services:"));
    }

    #[test]
    fn compose_macos_uses_bridge_network() {
        let selections = ServiceSelections {
            searxng: true,
            crawl4ai: false,
            docling_container: false,
        };
        let compose = generate_compose(&selections, false); // not linux
        assert!(!compose.contains("network_mode: host"));
        assert!(compose.contains("host.docker.internal"));
    }

    #[test]
    fn parse_service_flag() {
        assert!(matches!(
            ServiceChoice::from_flag("local"),
            Ok(ServiceChoice::NixNative)
        ));
        assert!(matches!(
            ServiceChoice::from_flag("container"),
            Ok(ServiceChoice::Container)
        ));
        assert!(matches!(
            ServiceChoice::from_flag("skip"),
            Ok(ServiceChoice::Skip)
        ));
        let remote = ServiceChoice::from_flag("remote:http://example.com").unwrap();
        assert!(matches!(remote, ServiceChoice::Remote(url) if url == "http://example.com"));
    }
}
```

- [ ] **Step 2: Run tests — verify they fail**

Run: `cargo test --lib onboarding::services`

- [ ] **Step 3: Implement `services.rs`**

Core types and functions:

```rust
/// Which container services to include in the compose file.
#[derive(Debug, Default)]
pub struct ServiceSelections {
    pub searxng: bool,
    pub crawl4ai: bool,      // always includes chrome
    pub docling_container: bool,
}
```

Functions:

- `prompt_embeddings(env: &DetectedEnvironment, flag: Option<&str>) -> Result<(ServiceChoice, Option<String>)>`
  — shows description + detection state, offers nix/remote/skip (or "Use existing" if
  detected on :11434). Smart default: skip if low_memory. If not skipped, prompts for
  embedding model name (default: `qwen3-embedding:8b`). Returns (choice, model_name).
- `prompt_search(env: &DetectedEnvironment, flag: Option<&str>) -> Result<SearchChoice>`
  — shows description, offers local/brave/remote/skip.
- `prompt_crawl(env: &DetectedEnvironment, flag: Option<&str>) -> Result<ServiceChoice>`
  — shows description, offers container/remote/skip.
- `prompt_docling(env: &DetectedEnvironment, flag: Option<&str>) -> Result<ServiceChoice>`
  — shows description, offers nix/container/remote/skip. Smart default: skip if
  low_memory.
- `install_nix_package(package: &str) -> Result<()>` — runs
  `nix profile install nixpkgs#<package>` with a cliclack spinner.
- `generate_compose(selections: &ServiceSelections, is_linux: bool) -> String` — builds
  the compose YAML from `include_str!`'d templates. On Linux uses `network_mode: host`,
  on macOS uses bridge + `host.docker.internal`.
- `write_compose_and_configs(workspace: &Path, selections: &ServiceSelections, is_linux: bool) -> Result<()>`
  — writes `services/docker-compose.yml` and `services/searxng-settings.yml`.

Each prompt function displays:

1. Section header with service name
2. 2-3 sentence description of what the service does and why the GHOST needs it
3. Link to project homepage
4. Detection state (what was found in Phase 0)
5. The picker with smart defaults

- [ ] **Step 4: Run tests — verify they pass**

Run: `cargo test --lib onboarding::services`

- [ ] **Step 5: Wire into `execute()` in `init.rs`**

After Phase 2, add Phase 3:

```rust
// Phase 3: Services
let embeddings = services::prompt_embeddings(&env, args.embeddings.as_deref()).await?;
let search = services::prompt_search(&env, args.search.as_deref()).await?;
let crawl = services::prompt_crawl(&env, args.crawl.as_deref()).await?;
let docling = services::prompt_docling(&env, args.docling.as_deref()).await?;

// Install nix packages for selected native services
services::install_nix_packages(&embeddings, &docling).await?;
```

- [ ] **Step 6: Commit**

```
git add src/onboarding/services.rs src/onboarding/mod.rs src/cli/init.rs
git commit -m "feat: add service setup and compose generation to onboarding"
```

---

## Task 7: Config writing + diff display (`config_writer.rs`)

**Files:**

- Create: `src/onboarding/config_writer.rs`
- Modify: `src/onboarding/mod.rs` (add `pub mod config_writer;`)

- [ ] **Step 1: Write test for config generation**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_valid_config_toml() {
        let state = OnboardingState {
            provider: Some(ProviderChoice::OpenRouter),
            api_key: Some("sk-test".into()),
            model: Some("anthropic/claude-sonnet-4".into()),
            context_window: Some(200_000),
            discord_token: Some("token".into()),
            discord_user_id: Some("123456789012345678".into()),
            embeddings: Some(ServiceChoice::NixNative),
            embedding_model: Some("qwen3-embedding:8b".into()),
            search: Some(SearchChoice::SearxngLocal),
            crawl: Some(ServiceChoice::Container),
            docling: Some(ServiceChoice::NixNative),
        };

        let toml_str = generate_config_toml(&state);

        // Should parse as valid TOML
        let parsed: toml::Value = toml::from_str(&toml_str).unwrap();
        assert_eq!(
            parsed["models"]["primary"]["provider"].as_str(),
            Some("openrouter")
        );
        assert_eq!(
            parsed["discord"]["allowed_user_id"].as_str(),
            Some("123456789012345678")
        );
    }

    #[test]
    fn generates_env_file() {
        let state = OnboardingState {
            provider: Some(ProviderChoice::OpenRouter),
            api_key: Some("sk-or-test-123".into()),
            discord_token: Some("discord-token-123".into()),
            ..Default::default()
        };

        let env_str = generate_env(&state);
        assert!(env_str.contains("OPENROUTER_API_KEY=sk-or-test-123"));
        assert!(env_str.contains("DISCORD_BOT_TOKEN=discord-token-123"));
    }

    #[test]
    fn diff_shows_additions() {
        let old = "";
        let new = "[discord]\nallowed_user_id = \"123\"\n";
        let diff = compute_config_diff(old, new);
        assert!(diff.contains("+ [discord]"));
    }
}
```

- [ ] **Step 2: Implement `config_writer.rs`**

Core functions:

- `generate_config_toml(state: &OnboardingState) -> String` — builds config.toml from
  wizard state. Maps `ServiceChoice`/`SearchChoice` to the correct TOML sections
  (`[embeddings]`, `[web.search]`, `[web]`, `[[web.browsers]]`, `[docling]`).
- `generate_env(state: &OnboardingState) -> String` — builds .env with only secrets (API
  keys, tokens). Maps provider to correct env var name.
- `compute_config_diff(old: &str, new: &str) -> String` — line-by-line diff with `+`/`-`
  prefixes for display. Uses `similar` crate (already in Cargo.toml) for the diff
  algorithm.
- `display_diff_and_confirm(old_config: &str, new_config: &str) -> Result<bool>` — shows
  diff in terminal, asks "Apply these changes?".
- `write_config_files(config_dir: &Path, config_toml: &str, env: &str) -> Result<()>` —
  writes both files, preserving unmanaged .env keys if .env already exists.

- [ ] **Step 3: Run tests — verify they pass**

Run: `cargo test --lib onboarding::config_writer`

- [ ] **Step 4: Commit**

```
git add src/onboarding/config_writer.rs src/onboarding/mod.rs
git commit -m "feat: add config generation and diff display to onboarding"
```

---

## Task 8: Service file generation (`service_files.rs`)

**Files:**

- Create: `src/onboarding/service_files.rs`
- Modify: `src/onboarding/mod.rs` (add `pub mod service_files;`)
- Modify: `src/config_workspace.rs:8-36` (add `services/` directory)

- [ ] **Step 1: Write test for systemd unit generation**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ghost_daemon_unit_has_timeout() {
        let unit = generate_daemon_unit_systemd("/usr/bin/ghost", "/home/user/GHOST");
        assert!(unit.contains("TimeoutStopSec=120"));
        assert!(unit.contains("ExecStart=/usr/bin/ghost daemon"));
    }

    #[test]
    fn llama_server_unit_has_model_args() {
        let unit = generate_llama_server_unit_systemd(
            "/home/user/.nix-profile/bin/llama-server",
            "qwen3-embedding:8b",
        );
        assert!(unit.contains("llama-server"));
        assert!(unit.contains("--embedding"));
    }

    #[test]
    fn launchd_plist_has_keep_alive() {
        let plist = generate_daemon_plist("/usr/bin/ghost", "/Users/user/GHOST");
        assert!(plist.contains("<key>KeepAlive</key>"));
        assert!(plist.contains("<true/>"));
    }
}
```

- [ ] **Step 2: Implement `service_files.rs`**

Functions:

- `generate_daemon_unit_systemd(exe: &str, workspace: &str) -> String` — ghost-daemon
  unit with `TimeoutStopSec=120`, `Restart=on-failure`, PATH including nix profile
- `generate_llama_server_unit_systemd(exe: &str, model: &str) -> String` — llama-server
  unit with embedding-specific flags
- `generate_docling_unit_systemd(exe: &str) -> String` — docling-serve unit
- `generate_daemon_plist(exe: &str, workspace: &str) -> String` — macOS launchd plist
  with KeepAlive + RunAtLoad
- (Similar plist generators for llama-server and docling)
- `install_service_files(platform: &Platform, state: &OnboardingState, exe: &str, workspace: &str) -> Result<Vec<String>>`
  — writes all applicable service files, returns list of installed file paths
- `ensure_linger_enabled() -> Result<()>` — moved from old `init.rs`, runs
  `loginctl enable-linger`
- `stable_exe_path() -> Result<String>` — moved from old `init.rs`, resolves binary path

Also: update `src/config_workspace.rs` to add `"services"` to the directory list in
`bootstrap_workspace_dirs`.

- [ ] **Step 3: Run tests — verify they pass**

Run: `cargo test --lib onboarding::service_files`

- [ ] **Step 4: Commit**

```
git add src/onboarding/service_files.rs src/onboarding/mod.rs src/config_workspace.rs
git commit -m "feat: add service file generation (systemd/launchd) to onboarding"
```

---

## Task 9: Health checks (`health.rs`)

**Files:**

- Create: `src/onboarding/health.rs`
- Modify: `src/onboarding/mod.rs` (add `pub mod health;`)

- [ ] **Step 1: Write test for health status formatting**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_result_display() {
        let result = HealthResult {
            service: "SearXNG".to_string(),
            detail: ":8080".to_string(),
            healthy: true,
        };
        let line = result.display_line();
        assert!(line.contains("SearXNG"));
        assert!(line.contains(":8080"));
    }
}
```

- [ ] **Step 2: Implement `health.rs`**

Types and functions:

- `HealthResult { service: String, detail: String, healthy: bool }`
- `check_all_services(state: &OnboardingState) -> Vec<HealthResult>` — probes each
  configured service (skip those set to Skip/Remote). 5s timeout per probe.
- `display_health_table(results: &[HealthResult])` — renders the status table with green
  checks / yellow warnings via cliclack.
- `prompt_start_daemon(flag: bool) -> Result<bool>` — "Start the ghost daemon now?"
  confirm prompt.
- `start_all_services(platform: &Platform, runtime: Option<&ContainerRuntime>, workspace: &Path) -> Result<()>`
  — starts native services (systemctl/launchctl) + container stack (compose up -d). Also
  `systemctl --user enable` for boot persistence.
- `trigger_first_message() -> Result<()>` — polls daemon health for up to 30s, then
  sends a chat turn via the daemon API or CLI. Warns but doesn't fail on error.

- [ ] **Step 3: Run tests — verify they pass**

Run: `cargo test --lib onboarding::health`

- [ ] **Step 4: Commit**

```
git add src/onboarding/health.rs src/onboarding/mod.rs
git commit -m "feat: add health checks and service launcher to onboarding"
```

---

## Task 10: Wizard orchestration (`wizard.rs`) + wire into `init.rs`

**Files:**

- Create: `src/onboarding/wizard.rs`
- Modify: `src/cli/init.rs`

The main orchestration lives in `wizard.rs` (not `init.rs`) to keep `init.rs` thin (CLI
parsing only) and `wizard.rs` under the 500 LoC guideline. `wizard.rs` owns the phase
sequencing, existing config pre-fill, retry loops, and the `[h]` hotkey.

**Existing config pre-fill**: When updating an existing config, load it via
`crate::config::load()` and pass the resolved `Config` to each prompt function. Prompt
functions accept an `Option<&str>` for the pre-filled value (from existing config) in
addition to the `Option<&str>` for the CLI flag. CLI flag takes precedence over
pre-fill. Pre-filled values shown as defaults the user can accept with Enter.

**`[h]` hotkey**: cliclack's `Select` and `Input` prompts don't natively support
arbitrary hotkeys. Implementation approach: wrap each prompt in a loop that catches a
sentinel value (e.g., if the user types "h" or "help" in a text prompt, or selects a
"Need help? [h]" option appended to select menus). When triggered, call
`agent::run_agent_session()`, then re-display the prompt. This is simpler and more
reliable than trying to intercept raw keypresses.

- [ ] **Step 1: Implement `wizard.rs` with the full phase flow**

```rust
pub async fn execute(args: InitArgs) -> Result<(), GhostError> {
    // Phase 0: Detection
    let env = detect::detect().await;
    if !env.nix_installed { /* exit with Determinate URL */ }
    display_detection_results(&env);

    // Handle existing config
    let existing_config = /* load if exists, offer update/fresh/cancel */;

    // Phase 1: Provider
    let provider = provider::prompt_provider(args.provider.as_deref())?;
    let api_key = provider::prompt_api_key(&provider, args.api_key.as_deref()).await?;
    let model = provider::prompt_model(&provider, args.model.as_deref())?;
    let context_window = provider::prompt_context_window(args.context_window)?;
    provider::validate_provider(&provider, api_key.as_deref(), &model).await?;

    // Phase 2: Discord
    let (discord_token, discord_user_id) = discord::prompt_discord(
        args.discord_token.as_deref(), args.discord_user.as_deref(),
    ).await?;

    // Phase 3: Services
    let embeddings = services::prompt_embeddings(&env, args.embeddings.as_deref()).await?;
    let search = services::prompt_search(&env, args.search.as_deref()).await?;
    let crawl = services::prompt_crawl(&env, args.crawl.as_deref()).await?;
    let docling = services::prompt_docling(&env, args.docling.as_deref()).await?;
    services::install_nix_packages(&embeddings, &docling).await?;

    // Build state
    let state = OnboardingState { provider, api_key, model, context_window,
        discord_token, discord_user_id, embeddings, search, crawl, docling };

    // Phase 4: Write config + install services
    let config_toml = config_writer::generate_config_toml(&state);
    let env_file = config_writer::generate_env(&state);
    config_writer::display_diff_and_confirm(&existing_config, &config_toml)?;
    config_writer::write_config_files(&config_dir, &config_toml, &env_file)?;

    // Bootstrap workspace
    let config = crate::config::load()?;
    crate::config_workspace::bootstrap_workspace(&config)?;

    // Write compose file
    let selections = services::build_selections(&state);
    services::write_compose_and_configs(&config.workspace, &selections, env.platform.is_linux())?;

    // Install service files
    let exe = service_files::stable_exe_path()?;
    service_files::install_service_files(&env.platform, &state, &exe, &config.workspace)?;
    if env.platform.is_linux() {
        service_files::ensure_linger_enabled()?;
    }

    // Phase 5: Health checks + launch
    let results = health::check_all_services(&state).await;
    health::display_health_table(&results);

    let should_start = args.start || health::prompt_start_daemon(args.start)?;
    if should_start {
        health::start_all_services(&env.platform, env.container_runtime.as_ref(), &config.workspace).await?;
        health::trigger_first_message().await?;
    }

    // Outro
    cliclack::outro("Setup complete! Your GHOST is running.")?;
    // Print next steps box

    Ok(())
}
```

- [ ] **Step 2: Run `cargo check`**

Expected: compiles with all phases wired.

- [ ] **Step 3: Manual smoke test**

Run: `cargo run -- init --help` Then: `cargo run -- init` (interactive mode) — verify
the full flow works end to end.

- [ ] **Step 4: Commit**

```
git add src/cli/init.rs
git commit -m "feat: wire all onboarding phases together in ghost init"
```

---

## Task 11: Services skill + extras

**Files:**

- Create: `assets/skills/services/skill.md`
- Create: `assets/skills/services/observability.md`
- Create: `assets/skills/services/tailscale.md`

- [ ] **Step 1: Write the main services skill**

Create `assets/skills/services/skill.md` following agentskills.io format. Content covers
architecture, file layout, common operations, health checking, adding/removing services,
reconfiguring, and nix garbage collection. See spec for full content outline.

- [ ] **Step 2: Write the observability extra**

Create `assets/skills/services/observability.md`. Covers SigNoz stack overview,
reference compose file (`docker-compose.signoz.yml`), OTEL configuration, start/stop
commands, dashboard basics.

- [ ] **Step 3: Write the tailscale extra**

Create `assets/skills/services/tailscale.md`. Covers what Tailscale provides,
installation link, setup commands, exposing services, ACL considerations.

- [ ] **Step 4: Verify bundling works**

Run: `cargo build` Expected: `build.rs` picks up new files in `assets/skills/services/`
and bundles them.

- [ ] **Step 5: Commit**

```
git add assets/skills/services/
git commit -m "feat: add services skill with observability and tailscale extras"
```

---

## Task 12: Onboarding agent (`agent.rs`)

**Files:**

- Create: `src/onboarding/agent.rs`
- Create: `assets/onboarding-agent-prompt.md`
- Modify: `src/onboarding/mod.rs` (add `pub mod agent;`)

- [ ] **Step 1: Write the onboarding agent system prompt**

Create `assets/onboarding-agent-prompt.md`:

```markdown
You are the GHOST onboarding assistant. Your role is to help the user understand the
setup process and make decisions about their configuration.

You are embedded in the `ghost init` wizard. The user pressed [h] to ask for help.
Answer their question, then they'll press [q] to return to the wizard.

## What You Know

- GHOST is a personal AI agent that communicates via Discord
- It uses several services: LLM providers, embeddings (llama.cpp), web search (SearXNG),
  web fetch (Crawl4AI + Chrome), and document processing (Docling)
- Services can be local (nix-installed or container) or remote
- Configuration lives in ~/.config/ghost/config.toml and .env

## Guidelines

- Be concise — the user is in the middle of setup, not a chat session
- Explain what each service does and why the GHOST needs it
- Help with tradeoffs: local vs remote, resource requirements
- If asked about something outside onboarding, briefly answer and suggest they revisit
  after setup is complete
- Do NOT modify any files or run any commands
```

- [ ] **Step 2: Write test for state summary building**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::onboarding::*;

    #[test]
    fn state_summary_includes_configured_fields() {
        let state = OnboardingState {
            provider: Some(ProviderChoice::OpenRouter),
            model: Some("test-model".into()),
            ..Default::default()
        };
        let summary = build_state_summary(&state, "Services");
        assert!(summary.contains("openrouter"));
        assert!(summary.contains("test-model"));
        assert!(summary.contains("Services"));
    }

    #[test]
    fn state_summary_shows_remaining() {
        let state = OnboardingState::default(); // nothing configured
        let summary = build_state_summary(&state, "Provider");
        assert!(summary.contains("Remaining"));
    }
}
```

- [ ] **Step 3: Implement `agent.rs`**

Functions:

- `OnboardingAgent::new(provider: &ProviderChoice, api_key: Option<&str>, model: &str) -> Self`
  — initializes with validated provider credentials from Phase 1.
- `OnboardingAgent::chat(&self, state_summary: &str, user_input: &str) -> Result<String>`
  — sends a chat completion with system prompt + dynamic state context + user question.
  Returns the assistant response text.
- `run_agent_session(agent: &OnboardingAgent, state: &OnboardingState, current_phase: &str) -> Result<()>`
  — interactive loop: show prompt, read input, display response. Exits on `q` or empty
  input. Uses `cliclack` section framing for the chat display.
- `build_state_summary(state: &OnboardingState, phase: &str) -> String` — formats
  current wizard state as a text block for context injection.

- [ ] **Step 3: Commit**

```
git add src/onboarding/agent.rs assets/onboarding-agent-prompt.md src/onboarding/mod.rs
git commit -m "feat: add on-demand onboarding assistant"
```

---

## Task 13: Integration test (non-interactive mode)

**Files:**

- Create: test in appropriate location (see @testing skill for conventions)

- [ ] **Step 1: Write an integration test for non-interactive `ghost init`**

This test exercises the full flow with all CLI flags, verifying it produces valid config
files without any interactive prompts. It should:

1. Set up a temp directory for config and workspace
2. Run `ghost init` with all flags via the Rust API (not subprocess)
3. Assert `config.toml` was written and parses correctly
4. Assert `.env` was written with the right keys
5. Assert `services/docker-compose.yml` was written
6. Assert service file locations are correct

This test should be gated behind `#[cfg(feature = "live-tests")]` since it touches the
filesystem and potentially nix. Read @testing skill for test harness conventions.

- [ ] **Step 2: Run the test**

Run: `cargo test --features live-tests test_ghost_init_noninteractive`

- [ ] **Step 3: Commit**

```
git add tests/
git commit -m "test: add integration test for non-interactive ghost init"
```

---

## Task 14: Documentation

**Files:**

- Create: `docs/src/content/docs/getting-started/onboarding.md`
- Create: `docs/src/content/docs/getting-started/services.md`
- Modify: `docs/astro.config.mjs:30-48` (add sidebar entries)
- Modify: `docs/src/content/docs/getting-started/installation.mdx` (link to onboarding)
- Modify: `docs/src/content/docs/reference/dependencies.md` (update for new service
  management approach)

Two new pages: a short CLI-driven onboarding guide and a more detailed services
reference. Read the `/docs` skill before writing — it covers formatting rules,
terminology (`GHOST`/`OPERATOR` in all caps), and build verification.

- [ ] **Step 1: Write the onboarding page**

Create `docs/src/content/docs/getting-started/onboarding.md`:

```markdown
---
title: Onboarding
description: Set up your GHOST with the interactive setup wizard.
---

After installing the GHOST binary, run the onboarding wizard to configure everything:

## Quick Start

    ghost init

The wizard walks you through:

1. **LLM provider** — pick a provider, enter your API key, choose a model
2. **Discord** — create a bot and connect it to your server
3. **Services** — set up embeddings, web search, web fetch, and document processing
   (locally or remotely)

At the end, your GHOST starts and sends you a message on Discord.

## Non-Interactive Mode

For automated deployments, pass all options as flags:

    ghost init \
      --provider openrouter \
      --api-key "$OPENROUTER_API_KEY" \
      --model "anthropic/claude-sonnet-4" \
      --context-window 200000 \
      --discord-token "$DISCORD_BOT_TOKEN" \
      --discord-user "$DISCORD_USER_ID" \
      --embeddings local \
      --search local \
      --crawl local \
      --docling local \
      --start

## Re-running

Run `ghost init` again at any time to reconfigure. It detects your existing
configuration and offers to update it — showing a diff of all changes before applying.

## Need Help During Setup?

Press **h** at any prompt to ask the onboarding assistant for help. It uses your
configured LLM to answer questions about the setup process (available after the provider
step completes).

## Next Steps

- [Services](/getting-started/services/) — how the service stack works and how your
  GHOST manages it
- [Configuration](/getting-started/configuration/) — config.toml and .env reference
- [Workspace](/getting-started/workspace/) — what's in your GHOST's workspace directory
```

Keep it short. The wizard itself provides all the context the user needs.

- [ ] **Step 2: Write the services page**

Create `docs/src/content/docs/getting-started/services.md`:

```markdown
---
title: Services
description:
  How GHOST's service stack works — native services, containers, and how to manage them.
---

Your GHOST relies on several services to function. The onboarding wizard (`ghost init`)
sets them up, but this page explains how they work and how to manage them afterward.

## Architecture

Services come in two flavors:

### Native Services (nix + systemd/launchd)

Installed via `nix profile install` and managed as system services.

| Service           | Binary          | Purpose                          |
| ----------------- | --------------- | -------------------------------- |
| **ghost-daemon**  | `ghost`         | The GHOST itself                 |
| **llama-server**  | `llama-server`  | Embedding generation (llama.cpp) |
| **docling-serve** | `docling-serve` | PDF/document processing          |

On Linux, these run as systemd user services:

    systemctl --user status ghost-daemon llama-server docling-serve
    systemctl --user restart llama-server

On macOS, they run as launchd agents:

    launchctl list | grep com.ghost

### Container Services (podman/docker)

Managed via a single Docker Compose file at `<workspace>/services/docker-compose.yml`.

| Service      | Image                     | Purpose                         |
| ------------ | ------------------------- | ------------------------------- |
| **SearXNG**  | `searxng/searxng`         | Web search (meta search engine) |
| **Crawl4AI** | `unclecode/crawl4ai`      | Web page extraction             |
| **Chrome**   | `chromedp/headless-shell` | Headless browser for Crawl4AI   |

Common operations:

    # Status
    podman compose -f ~/GHOST/services/docker-compose.yml ps

    # Restart all
    podman compose -f ~/GHOST/services/docker-compose.yml restart

    # View logs
    podman compose -f ~/GHOST/services/docker-compose.yml logs -f searxng

    # Stop everything
    podman compose -f ~/GHOST/services/docker-compose.yml down

## File Layout

    ~/.config/ghost/
    ├── config.toml              # Configuration
    └── .env                     # Secrets (API keys, tokens)

    ~/GHOST/services/
    ├── docker-compose.yml       # Container stack
    └── searxng-settings.yml     # SearXNG configuration

    ~/.config/systemd/user/      # Linux
    ├── ghost-daemon.service
    ├── llama-server.service
    └── docling-serve.service

## Service Details

### Embeddings (llama-server)

Converts text into numerical vectors for semantic search. Your GHOST uses these vectors
to find relevant notes and references even when exact words don't match.

- **Model**: `qwen3-embedding:8b` (configurable in `config.toml`)
- **Port**: 11434
- **Config section**: `[embeddings]`

### Web Search (SearXNG)

Self-hosted meta search engine. Aggregates results from Google, Bing, DuckDuckGo, and
others — no API keys needed.

- **Port**: 8080
- **Config section**: `[web.search]`
- **Settings**: `<workspace>/services/searxng-settings.yml`

### Web Fetch (Crawl4AI + Chrome)

Reads web pages and converts them to clean markdown. Crawl4AI renders JavaScript-heavy
pages using a headless Chrome instance.

- **Crawl4AI port**: 11235
- **Chrome port**: 9222 (CDP)
- **Config section**: `[web]` (`crawl4ai_url`, `[[web.browsers]]`)

### Document Processing (Docling)

Converts PDFs, Word documents, and presentations to markdown. Handles OCR, table
extraction, and complex layouts.

- **Port**: 5001
- **Config section**: `[docling]`

## Optional: Observability (SigNoz)

SigNoz gives you distributed tracing, metrics, and logs for your GHOST via
OpenTelemetry. It's not set up by the wizard, but your GHOST knows how to help — ask it
about the **services** skill's observability extra.

Quick setup:

1. Ask your GHOST to read the services skill's observability extra
2. It will guide you through deploying the SigNoz stack and configuring
   `OTEL_EXPORTER_OTLP_ENDPOINT`

## Optional: Tailscale

Tailscale provides secure remote access to your GHOST without opening ports. Your GHOST
can help — ask it about the **services** skill's tailscale extra.

## Troubleshooting

### A service won't start

Check its logs:

    # Native service
    journalctl --user -u llama-server -f

    # Container service
    podman compose -f ~/GHOST/services/docker-compose.yml logs crawl4ai

### Reconfigure everything

    ghost init

This re-runs the wizard with your existing values pre-filled.

### Nix garbage collection

Nix stores grow over time. Clean up old generations periodically:

    nix-collect-garbage -d
```

- [ ] **Step 3: Add sidebar entries to `astro.config.mjs`**

In the `Getting Started` sidebar section, add the two new pages after the Installation
group and before Configuration:

```javascript
{
  label: "Getting Started",
  items: [
    {
      label: "Installation",
      items: [
        { label: "Overview", slug: "getting-started/installation" },
        { label: "macOS", slug: "getting-started/install-macos" },
        { label: "Linux", slug: "getting-started/install-linux" },
        { label: "From Source", slug: "getting-started/install-source" },
      ],
    },
    { label: "Onboarding", slug: "getting-started/onboarding" },
    { label: "Services", slug: "getting-started/services" },
    { label: "Configuration", slug: "getting-started/configuration" },
    { label: "Workspace", slug: "getting-started/workspace" },
  ],
},
```

- [ ] **Step 4: Update installation.mdx to link to onboarding**

At the bottom of `docs/src/content/docs/getting-started/installation.mdx`, add a note
pointing to the onboarding page:

```markdown
## After Installing

Run the onboarding wizard to configure your GHOST:

    ghost init

See [Onboarding](/getting-started/onboarding/) for details.
```

- [ ] **Step 5: Update dependencies.md**

Update `docs/src/content/docs/reference/dependencies.md` to reflect the new service
management approach:

- Replace "Ollama" section with "llama-server (llama.cpp)" — installed via nix, not
  standalone Ollama
- Update the note at top: services are now managed by `ghost init` via nix and
  podman/docker compose, not manually
- Add Chrome/headless-shell entry (currently missing)
- Update command examples to use `podman compose` instead of `docker run`

- [ ] **Step 6: Build and verify docs**

Run: `cd docs && npm run build` Expected: builds with no errors. Check the sidebar shows
the new pages in the right order.

- [ ] **Step 7: Commit**

```
git add docs/
git commit -m "docs: add onboarding and services pages"
```

---

## Task 15: Final verification + cleanup

- [ ] **Step 1: Run `just ci`**

Run: `just ci` Expected: all format, check, clippy, and test checks pass.

- [ ] **Step 2: Manual end-to-end test**

Run `cargo run -- init` interactively and verify:

- Phase 0 detection shows correct results
- Provider picker works, API validation succeeds
- Discord guide box renders beautifully
- Service prompts show descriptions and links
- Config diff displays correctly
- Services start successfully
- Health checks show green
- Daemon starts and sends first Discord message

- [ ] **Step 3: Test scriptable mode**

Run with all flags to verify non-interactive mode produces the same result.

- [ ] **Step 4: Commit any final fixes**

```
git commit -m "chore: final cleanup for ghost init onboarding"
```
