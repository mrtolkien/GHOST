# WASM Agent Runtime — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or superpowers:executing-plans
> to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the Lua agent runtime (mlua) with WASM components (wasmtime). Agents
become Rust crates compiled to `wasm32-wasip2`. A `ghost-agent` SDK crate provides
macros that eliminate boilerplate.

**Architecture:** WIT defines the agent↔host contract. wasmtime loads compiled `.wasm`
components and implements host imports (ctx methods) as async functions. Ghost scaffolds
each agent as a standalone Rust crate; a shared `CARGO_TARGET_DIR` provides build
caching across agents.

**Tech Stack:** wasmtime (component-model + async), wit-bindgen, cargo-component,
ghost-agent SDK crate (proc macros + types).

**Spec:** `backlog/tasks/5-management-safety/wasm-agents.md`

---

## Phase 1: Minimal End-to-End (prove the architecture)

Get ONE agent running as a WASM component with a manually-written guest (no SDK macros
yet). This validates: WIT design, wasmtime hosting, async ctx imports, component
loading.

### Task 1: WIT definition + wasmtime scaffold

**Files:**

- Create: `wit/agent.wit`
- Create: `src/agents/runtime.rs`
- Modify: `src/agents/mod.rs`
- Modify: `Cargo.toml` (add wasmtime)

- [ ] **Step 1: Add wasmtime dependency**

Add to `Cargo.toml`:

```toml
wasmtime = { version = "43", features = ["component-model", "async"] }
```

- [ ] **Step 2: Write the WIT interface**

Create `wit/agent.wit` with the full interface from the spec: `types` interface
(message, build-result, tool-def, reasoning-effort, agent-config, cache-stats,
interface-session), `context` interface (all ctx methods), and `agent` world (init,
build, handle-tool, post-completion, on-resume exports).

Reference: spec section "WIT interface" has the complete definition.

- [ ] **Step 3: Create runtime.rs with bindgen + AgentRuntime**

```rust
// src/agents/runtime.rs
use std::collections::HashMap;
use std::path::Path;
use wasmtime::component::{bindgen, Component, Linker};
use wasmtime::{Config, Engine, Store};

bindgen!({
    world: "agent",
    path: "wit/agent.wit",
    async: true,
});

pub struct AgentRuntime {
    engine: Engine,
    linker: Linker<HostState>,
    cache: HashMap<String, wasmtime::component::InstancePre<HostState>>,
}
```

Implement `AgentRuntime::new()` — create engine with async + component model enabled,
create linker, register `context::add_to_linker`. Implement `load()` to compile a
component from file and cache as `InstancePre`. Implement `call_init()`, `call_build()`,
`call_handle_tool()`, `call_post_completion()`, `call_on_resume()` — each creates a
`Store<HostState>`, instantiates from `InstancePre`, calls the export.

- [ ] **Step 4: Define HostState struct**

```rust
pub struct HostState {
    pub db: crate::db::GhostDb,
    pub session_id: String,
    pub agent_slug: String,
    pub trigger_session_id: Option<String>,
    pub workspace: std::path::PathBuf,
    pub agent_dir: std::path::PathBuf,
    pub tool_manager: std::sync::Arc<crate::tools::ToolManager>,
    pub spawn_requests: Vec<SpawnRequest>,
}

pub struct SpawnRequest {
    pub name: String,
    pub args_json: String,
}
```

- [ ] **Step 5: Add module to agents/mod.rs, verify it compiles**

Run: `cargo check`

- [ ] **Step 6: Commit**

```
feat: add WIT definition and wasmtime runtime scaffold
```

### Task 2: Implement context::Host

**Files:**

- Create: `src/agents/wasm_host.rs`
- Modify: `src/agents/mod.rs`

All ctx methods the WIT imports define. Each is an async function on `HostState` that
delegates to existing DB/tool infrastructure.

- [ ] **Step 1: Implement identity methods**

`session_id()`, `agent_slug()`, `trigger_session_id()`, `workspace()` — return cloned
fields from HostState.

- [ ] **Step 2: Implement state persistence methods**

`get()`, `set()`, `delete()` — delegate to `db::agent_state` module. Use existing
`get_agent_state`, `set_agent_state`, `delete_agent_state` functions. Handle errors by
returning `None` / logging.

- [ ] **Step 3: Implement session/transcript methods**

`count_messages_since()`, `filter_transcript()`, `load_diary_today()`,
`list_messages()`, `list_interface_sessions()` — delegate to existing `db::sessions`
functions. `load_diary_today()` reads from the diary directory on disk.

- [ ] **Step 4: Implement file methods**

`read_file()` — resolve path relative to `agent_dir`, validate it doesn't escape
workspace (canonicalize + starts_with check), read with `std::fs::read_to_string`.
`load_skill()` — read from `$WORKSPACE/skills/{name}/skill.md`, strip YAML frontmatter.

- [ ] **Step 5: Implement tool execution**

`call_tool()` — delegate to `tool_manager.execute()`. Deserialize args_json, serialize
result. Ghost enforces the agent's tools list at the `ToolManager` level.

- [ ] **Step 6: Implement web cache + spawning**

`curate_web_cache()` — delegate to existing reflection module. `spawn_agent()` — push to
`self.spawn_requests`.

- [ ] **Step 7: Verify it compiles**

Run: `cargo check`

- [ ] **Step 8: Commit**

```
feat: implement context::Host for WASM agent imports
```

### Task 3: Test with a hand-written WASM agent

**Files:**

- Create: `tests/fixtures/test-agent/Cargo.toml`
- Create: `tests/fixtures/test-agent/src/lib.rs`
- Create: `tests/wasm_runtime.rs`

Build a minimal agent by hand (raw wit_bindgen, no SDK) to test the runtime end-to-end.

- [ ] **Step 1: Create test agent crate**

`tests/fixtures/test-agent/Cargo.toml`:

```toml
[package]
name = "test-agent"
version = "0.1.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
wit-bindgen = "0.41"
```

`tests/fixtures/test-agent/src/lib.rs`:

```rust
wit_bindgen::generate!({
    world: "agent",
    path: "../../../wit/agent.wit",
});

struct TestAgent;

impl Guest for TestAgent {
    fn init() -> types::AgentConfig {
        types::AgentConfig {
            name: "test-agent".to_string(),
            description: "A test agent".to_string(),
            model: None,
            reasoning_effort: None,
            max_iterations: 5,
            tools: vec!["file_read".to_string()],
            skills: vec![],
            custom_tools: vec![],
            compaction_instructions: None,
            has_post_completion: false,
            has_on_resume: false,
        }
    }

    fn build(args_json: String) -> types::BuildResult {
        types::BuildResult {
            system_prompt: "You are a test agent.".to_string(),
            messages: vec![types::Message {
                role: "user".to_string(),
                content: format!("Test: {args_json}"),
            }],
        }
    }

    fn handle_tool(_name: String, _args_json: String) -> Result<String, String> {
        Err("no custom tools".to_string())
    }

    fn post_completion() {}

    fn on_resume(_prompt: String) -> Option<types::BuildResult> {
        None
    }
}

export!(TestAgent);
```

- [ ] **Step 2: Compile the test agent to WASM**

Add a build script or test helper that runs:

```bash
cargo component build --manifest-path tests/fixtures/test-agent/Cargo.toml \
    --target wasm32-wasip2 --release
```

Verify the `.wasm` file is produced.

- [ ] **Step 3: Write runtime integration test**

`tests/wasm_runtime.rs` (gated behind `live-tests` feature since it needs compilation
toolchain):

```rust
#[cfg(feature = "live-tests")]
#[tokio::test]
async fn test_wasm_agent_init() {
    // Build the test agent WASM (helper function)
    let wasm_path = build_test_agent();

    let mut runtime = AgentRuntime::new().unwrap();
    runtime.load("test-agent", &wasm_path).unwrap();

    let state = make_test_host_state();
    let config = runtime.call_init("test-agent", state).await.unwrap();

    assert_eq!(config.name, "test-agent");
    assert_eq!(config.max_iterations, 5);
    assert_eq!(config.tools, vec!["file_read"]);
}

#[cfg(feature = "live-tests")]
#[tokio::test]
async fn test_wasm_agent_build() {
    let wasm_path = build_test_agent();
    let mut runtime = AgentRuntime::new().unwrap();
    runtime.load("test-agent", &wasm_path).unwrap();

    let state = make_test_host_state();
    let result = runtime
        .call_build("test-agent", state, r#"{"prompt":"hello"}"#)
        .await
        .unwrap();

    assert_eq!(result.system_prompt, "You are a test agent.");
    assert_eq!(result.messages[0].role, "user");
    assert!(result.messages[0].content.contains("hello"));
}
```

- [ ] **Step 4: Run tests, verify they pass**

Run: `cargo test --features live-tests test_wasm_agent`

- [ ] **Step 5: Commit**

```
test: end-to-end WASM agent runtime integration test
```

---

## Phase 2: ghost-agent SDK crate

Build the SDK that eliminates WIT boilerplate, set up CI/CD, and publish to crates.io.
The SDK must be published before Phase 4 (agent conversion) — agents depend on it by
version from crates.io.

### Task 4: SDK crate scaffold + types

**Files:**

- Create: `crates/ghost-agent/Cargo.toml`
- Create: `crates/ghost-agent/src/lib.rs`
- Create: `crates/ghost-agent/src/types.rs`
- Create: `crates/ghost-agent/src/helpers.rs`
- Modify: root `Cargo.toml` to become workspace

- [ ] **Step 1: Convert root to workspace**

Wrap existing Ghost crate in a workspace. Root `Cargo.toml` gets:

```toml
[workspace]
members = [".", "crates/ghost-agent", "crates/ghost-agent-macros"]
resolver = "3"
```

Verify `cargo check` still works.

- [ ] **Step 2: Create ghost-agent crate**

`crates/ghost-agent/Cargo.toml`:

```toml
[package]
name = "ghost-agent"
version = "0.1.0"
edition = "2024"
description = "SDK for building Ghost WASM agents"

[dependencies]
ghost-agent-macros = { path = "../ghost-agent-macros" }
wit-bindgen = "0.41"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

- [ ] **Step 3: Write types.rs**

`BuildResult`, `Message`, `Ctx` (thin wrapper around WIT context imports), `Args`
(wrapper around `HashMap<String, String>` with index support). Include
`BuildResult::new` convenience constructor. Include `user()` and `system()` helper
functions that create `Message` values.

- [ ] **Step 4: Write lib.rs**

Public prelude module re-exporting everything an agent needs: `agent!`, `BuildResult`,
`Ctx`, `Args`, `Schema`, `user`, `system`, `Deserialize`, `serde_json`.

Embed the WIT as a constant:

```rust
pub const WIT: &str = include_str!("../../wit/agent.wit");
```

(The wit/ directory at repo root is shared between Ghost and the SDK.)

- [ ] **Step 5: Verify it compiles**

Run: `cargo check -p ghost-agent`

- [ ] **Step 6: Commit**

```
feat: ghost-agent SDK crate scaffold with types
```

### Task 5: Schema derive macro

**Files:**

- Create: `crates/ghost-agent-macros/Cargo.toml`
- Create: `crates/ghost-agent-macros/src/lib.rs`
- Create: `crates/ghost-agent-macros/src/schema.rs`

Derive macro that generates JSON Schema from Rust types. Used by `#[tool]` to
auto-generate tool parameter schemas.

- [ ] **Step 1: Create proc-macro crate**

`crates/ghost-agent-macros/Cargo.toml`:

```toml
[package]
name = "ghost-agent-macros"
version = "0.1.0"
edition = "2024"

[lib]
proc-macro = true

[dependencies]
proc-macro2 = "1"
quote = "1"
syn = { version = "2", features = ["full"] }
```

- [ ] **Step 2: Implement Schema derive**

The derive macro generates a `fn json_schema() -> String` method. Handle:

- `String` → `{"type": "string"}`
- `i32`/`u32`/`i64`/`u64` → `{"type": "integer"}`
- `f32`/`f64` → `{"type": "number"}`
- `bool` → `{"type": "boolean"}`
- `Vec<T>` → `{"type": "array", "items": <T::json_schema()>}`
- `Option<T>` → marks field as not required
- Named struct → `{"type": "object", "properties": {...}, "required": [...]}`
- Fieldless enum → `{"type": "string", "enum": [...]}`
- Respect `#[serde(default)]` as not required

- [ ] **Step 3: Write unit tests for Schema derive**

Test each type mapping. Test a struct with mixed required/optional fields. Test a
fieldless enum. Test nested structs.

- [ ] **Step 4: Run tests**

Run: `cargo test -p ghost-agent-macros`

- [ ] **Step 5: Commit**

```
feat: Schema derive macro for JSON Schema generation
```

### Task 6: agent! proc macro

**Files:**

- Modify: `crates/ghost-agent-macros/src/lib.rs`
- Create: `crates/ghost-agent-macros/src/agent.rs`

The wrapping macro that parses config + hooks + tools, generates
`wit_bindgen::generate!`, Guest trait impl, and `export!`.

- [ ] **Step 1: Define the macro's input syntax**

The macro accepts:

```
agent! {
    name: "...",
    description: "...",
    [model: "...",]
    [reasoning_effort: low|medium|high|xhigh,]
    [max_iterations: N,]
    [tools: ["...", ...],]
    [skills: ["...", ...],]
    [compaction: "...",]

    build(ctx, args) { ... }
    [post_completion(ctx) { ... }]
    [on_resume(ctx, prompt) { ... }]

    [#[tool(description = "...", terminal)]
    tool_name(ctx, input: InputType) -> String { ... }]
}
```

Parse config keys first, then function definitions. Detect which optional hooks are
present.

- [ ] **Step 2: Implement config parsing**

Parse the key-value pairs at the start of the macro body. Build an `AgentConfig` literal
from them, converting string literals and arrays.

- [ ] **Step 3: Implement hook detection and delegation**

Find `build`, `post_completion`, `on_resume` function bodies. Generate the Guest trait
impl that delegates to them. For absent hooks, generate no-ops. Set
`has_post_completion` / `has_on_resume` flags in the config accordingly.

- [ ] **Step 4: Implement #[tool] parsing and dispatch**

Find `#[tool(...)]` annotated functions. For each:

- Extract description and terminal flag from attributes
- Get the input type (second param after ctx)
- Generate a `ToolDef` entry in `custom_tools` using `<InputType>::json_schema()`
- Generate match arm in `handle_tool()` that deserializes args and calls the function

- [ ] **Step 5: Generate wit_bindgen + export**

The macro outputs:

```rust
wit_bindgen::generate!({
    world: "agent",
    inline: ghost_agent::WIT,
});

struct __Agent;
impl Guest for __Agent { ... }
export!(__Agent);
```

- [ ] **Step 6: Write integration test — compile a macro-based agent to WASM**

Create a test agent using `agent!` macro, compile to WASM, load with AgentRuntime, call
init() and build(), verify results match.

- [ ] **Step 7: Run test**

Run: `cargo test --features live-tests test_sdk_agent`

- [ ] **Step 8: Commit**

```
feat: agent! proc macro for WASM agent definitions
```

### Task 7: SDK release infrastructure + first publish

**Files:**

- Modify: `release-please-config.json`
- Modify: `.release-please-manifest.json`
- Create: `.github/workflows/publish-sdk.yml`

The SDK has its own version (starting at `0.1.0-alpha.1`), its own release-please
component, and a GitHub workflow that publishes to crates.io on tag. Both
`ghost-agent-macros` and `ghost-agent` must be published (macros first — ghost-agent
depends on it).

- [ ] **Step 1: Add SDK packages to release-please config**

`release-please-config.json` — add two new packages:

```json
{
  "packages": {
    ".": { ... existing ... },
    "crates/ghost-agent-macros": {
      "release-type": "rust",
      "component": "ghost-agent-macros",
      "include-component-in-tag": true,
      "bump-minor-pre-major": true,
      "prerelease": true,
      "prerelease-type": "alpha"
    },
    "crates/ghost-agent": {
      "release-type": "rust",
      "component": "ghost-agent",
      "include-component-in-tag": true,
      "bump-minor-pre-major": true,
      "prerelease": true,
      "prerelease-type": "alpha"
    }
  }
}
```

Update `.release-please-manifest.json`:

```json
{
  ".": "0.11.1",
  "crates/ghost-agent-macros": "0.1.0-alpha.1",
  "crates/ghost-agent": "0.1.0-alpha.1"
}
```

- [ ] **Step 2: Create crates.io publish workflow**

`.github/workflows/publish-sdk.yml`:

```yaml
name: Publish SDK to crates.io

on:
  push:
    tags:
      - "ghost-agent-macros-v*"
      - "ghost-agent-v*"

jobs:
  publish:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - uses: dtolnay/rust-toolchain@stable

      - name: Publish ghost-agent-macros
        if: startsWith(github.ref, 'refs/tags/ghost-agent-macros-v')
        run: cargo publish -p ghost-agent-macros
        env:
          CARGO_REGISTRY_TOKEN: ${{ secrets.CARGO_REGISTRY_TOKEN }}

      - name: Publish ghost-agent
        if: startsWith(github.ref, 'refs/tags/ghost-agent-v')
        run: cargo publish -p ghost-agent
        env:
          CARGO_REGISTRY_TOKEN: ${{ secrets.CARGO_REGISTRY_TOKEN }}
```

- [ ] **Step 3: Set CARGO_REGISTRY_TOKEN secret**

Add the crates.io API token as a repository secret named `CARGO_REGISTRY_TOKEN`.
Generate from https://crates.io/settings/tokens with `publish-new` and `publish-update`
scopes.

- [ ] **Step 4: Set initial versions in Cargo.toml files**

`crates/ghost-agent-macros/Cargo.toml`:

```toml
version = "0.1.0-alpha.1"
```

`crates/ghost-agent/Cargo.toml`:

```toml
version = "0.1.0-alpha.1"

[dependencies]
ghost-agent-macros = { version = "=0.1.0-alpha.1", path = "../ghost-agent-macros" }
```

The `path = ` ensures local development uses the workspace copy. The `version = `
ensures crates.io resolution works for published agents. Both are needed.

- [ ] **Step 5: Commit**

```
ci: add release-please + crates.io publish workflow for ghost-agent SDK
```

- [ ] **Step 6: STOP — hand off to OPERATOR for first publish**

**Do not proceed to Phase 3.** The OPERATOR must:

1. Set the `CARGO_REGISTRY_TOKEN` secret in GitHub repo settings
2. Run `cargo publish -p ghost-agent-macros` then `cargo publish -p ghost-agent`
   manually to claim the crate names on crates.io
3. Verify both crates appear on crates.io
4. Confirm agents can resolve the dependency: create a test Cargo.toml outside the
   workspace with `ghost-agent = "0.1.0-alpha.1"`, run `cargo check`
5. Signal that Phase 3 can proceed

**Resume from Task 8 after OPERATOR confirms the crates are published.**

---

## Phase 3: Wire into Ghost

Replace ScriptHost usage with AgentRuntime throughout the Ghost codebase.

### Task 8: Agent compilation orchestration

**Files:**

- Create: `src/agents/compiler.rs`
- Modify: `src/agents/mod.rs`

- [ ] **Step 1: Implement compile_agent()**

```rust
pub async fn compile_agent(
    agent_dir: &Path,
    workspace: &Path,
) -> Result<PathBuf, AgentError> {
    let target_dir = workspace.join(".agent-cache/target");
    let status = tokio::process::Command::new("cargo")
        .arg("component")
        .arg("build")
        .arg("--release")
        .arg("--manifest-path")
        .arg(agent_dir.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", &target_dir)
        .status()
        .await?;
    if !status.success() {
        return Err(AgentError::CompilationFailed { ... });
    }
    // Return path to .wasm artifact
    let name = read_crate_name(agent_dir)?;
    Ok(target_dir.join(format!("wasm32-wasip2/release/{name}.wasm")))
}
```

- [ ] **Step 2: Add CompilationFailed variant to AgentError**

- [ ] **Step 3: Implement wasm artifact caching**

Check `.wasm` mtime vs source file mtimes. Skip compilation if artifact is newer than
all source files. This avoids recompiling unchanged agents on every boot.

- [ ] **Step 4: Commit**

```
feat: agent compilation orchestration with caching
```

### Task 9: Replace loader.rs

**Files:**

- Modify: `src/agents/loader.rs`

The loader currently discovers agents by finding `agent.lua` files and loading config
via ScriptHost. Replace with: find `agent.rs` files (or `Cargo.toml` with
`crate-type = ["cdylib"]`), compile to WASM, load with AgentRuntime, call `init()`.

- [ ] **Step 1: Update resolve_agent_dir()**

Change search from `agent.lua` to `agent.rs` (or check for `Cargo.toml` presence). Keep
the same priority: `agents/{name}/` first, then recursive `skills/**/`.

- [ ] **Step 2: Update discover_agents()**

Same filesystem scan but looking for `agent.rs` instead of `agent.lua`. Extract agent
info by compiling + calling `init()`, or by parsing the `agent!` macro config from
source text (faster for discovery — avoids compilation for listing).

For discovery without compilation: regex-parse `name:` and `description:` from the
`agent!` block in `agent.rs`. This is a heuristic for `ghost agent list` — full
compilation happens when actually running the agent.

- [ ] **Step 3: Update load_agent()**

Replace `ScriptHost::new().load_config()` with: compile agent → load WASM →
`runtime.call_init()` → return AgentConfig.

- [ ] **Step 4: Update validate_agent()**

Validation = successful compilation + `init()` returning valid config. The compiler
catches type errors; the runtime catches WIT contract violations.

- [ ] **Step 5: Update tests in loader.rs**

Replace `setup_lua_agent()` helper with `setup_wasm_agent()` that writes `Cargo.toml` +
`agent.rs` and compiles to WASM.

- [ ] **Step 6: Commit**

```
refactor: replace Lua-based agent loader with WASM loader
```

### Task 10: Replace runner.rs hooks

**Files:**

- Modify: `src/agents/runner.rs`

The runner currently uses `ScriptHost` to call hooks (build, post_completion, on_resume)
and custom tool handlers. Replace with `AgentRuntime` calls.

- [ ] **Step 1: Replace setup_agent()**

Currently: `load_agent_with_host()` → `host.call_build()`. Replace with: compile agent →
`runtime.load()` → `runtime.call_build()`. The `AgentRuntime` replaces `ScriptHost` in
the agent execution pipeline.

- [ ] **Step 2: Replace hook calls in execute_agent()**

Replace `script_host.call_post_completion()` with `runtime.call_post_completion()`.
Replace `script_host.call_on_resume()` with `runtime.call_on_resume()`.

- [ ] **Step 3: Replace custom tool execution**

Currently: `LuaToolAdapter` wraps Lua handlers in the `Tool` trait. Replace with
`WasmToolAdapter` that calls `runtime.call_handle_tool(name, args_json)`.

- [ ] **Step 4: Update spawn request collection**

Currently: spawn requests are collected from `AgentContext.spawn_requests` (Arc<Mutex>).
Replace with: `HostState.spawn_requests` — collected after each hook call by draining
the Store's state.

- [ ] **Step 5: Update chat session hook calls**

In `src/chat/session.rs`: the `LuaAgentHandler` calls `script_host.call_pre_turn()` and
`script_host.call_on_end_turn()`. Since we're removing these hooks (nudges removed),
remove the `LuaAgentHandler` entirely. If pre_turn/on_end_turn are needed later, add
them back through the WIT.

- [ ] **Step 6: Run existing agent tests**

Run: `cargo test agent` and verify the tests that can be adapted pass. Some tests will
need updating in the next step.

- [ ] **Step 7: Commit**

```
refactor: replace ScriptHost with AgentRuntime in agent runner
```

### Task 11: Update scheduler and crontab

**Files:**

- Modify: `src/agents/scheduler.rs`
- Modify: `src/agents/crontab.rs`

- [ ] **Step 1: Update crontab parser**

Replace Lua-based crontab parsing with TOML:

```rust
#[derive(Deserialize)]
struct Crontab {
    schedule: Vec<CrontabEntry>,
}

#[derive(Deserialize)]
struct CrontabEntry {
    agent: String,
    cron: Option<String>,
    idle_minutes: Option<u64>,
}

pub fn load_crontab(workspace: &Path) -> Result<Vec<CrontabEntry>> {
    let path = workspace.join("agents/crontab.toml");
    let content = std::fs::read_to_string(&path)?;
    let crontab: Crontab = toml::from_str(&content)?;
    Ok(crontab.schedule)
}
```

- [ ] **Step 2: Update scheduler file watcher**

Change watched extensions from `.lua` to `.rs` and `.toml`. The debounce and hot-reload
logic stays the same.

- [ ] **Step 3: Convert crontab.lua → crontab.toml**

`assets/agents/crontab.toml`:

```toml
[[schedule]]
agent = "chat-reflection"
idle_minutes = 30
```

Remove `assets/agents/crontab.lua`.

- [ ] **Step 4: Commit**

```
refactor: TOML crontab, update scheduler for WASM agents
```

---

## Phase 4: Convert agents

Mechanical conversion of all 8 agents from Lua to Rust using the SDK.

### Task 12: Convert bundled agents

**Files:**

- Modify: all files under `assets/agents/` and `assets/skills/` that contain `agent.lua`
- Modify: `src/bundled.rs` and `build.rs` (update bundled file discovery)

- [ ] **Step 1: Convert chat-reflection**

Replace `assets/agents/chat-reflection/agent.lua` with:

- `assets/agents/chat-reflection/Cargo.toml`
- `assets/agents/chat-reflection/agent.rs`

Use the SDK `agent!` macro. Keep `prompt.md` unchanged. Reference spec examples for the
exact code.

- [ ] **Step 2: Convert deep-research**

Replace `assets/skills/deep-research/deep-research/agent.lua` with `Cargo.toml` +
`agent.rs`. Include the `ReportInput` and `Source` structs with `#[derive(Schema)]`.

- [ ] **Step 3: Convert deep-research-reflection**

Replace `assets/skills/deep-research/deep-research-reflection/agent.lua`.

- [ ] **Step 4: Convert coding-implementer**

Replace `assets/skills/superpowers/subagent-development/coding-implementer/agent.lua`.

- [ ] **Step 5: Convert coding-spec-reviewer**

Replace `assets/skills/superpowers/subagent-development/coding-spec-reviewer/agent.lua`.

- [ ] **Step 6: Convert coding-quality-reviewer**

Replace
`assets/skills/superpowers/subagent-development/coding-quality-reviewer/agent.lua`.

- [ ] **Step 7: Convert coding-reviewer**

Replace `assets/skills/superpowers/subagent-development/coding-reviewer/agent.lua`.

- [ ] **Step 8: Update build.rs / bundled.rs**

Update the asset bundling to include `Cargo.toml` + `agent.rs` instead of `agent.lua`.
The bundling mechanism (`include_str!` + install to workspace) stays the same — just
different files.

- [ ] **Step 9: Compile all agents, verify they produce valid WASM**

Run compilation for each agent. Load each with AgentRuntime, call `init()`, verify
config matches the Lua original.

- [ ] **Step 10: Commit**

```
feat: convert all 8 agents from Lua to WASM Rust
```

---

## Phase 5: Cleanup

### Task 13: Delete Lua infrastructure

**Files:**

- Delete: `src/scripting/` (host.rs, bindings.rs, custom_tools.rs, types.rs, mod.rs)
- Delete: `prompts/stdlib/` (nudges.lua, template.lua)
- Delete: `assets/agents/.types/ghost.lua`
- Delete: `.luarc.json`
- Delete: `.stylua.toml`
- Modify: `Cargo.toml` — remove `mlua`
- Modify: `src/agents/mod.rs` — remove `pub mod scripting` or any re-export
- Delete: `.agents/skills/lua-scripting/SKILL.md`

- [ ] **Step 1: Delete src/scripting/**

Remove the entire directory. Fix any compilation errors from dangling imports — the
modules that imported from `scripting` should now use `agents::runtime` instead.

- [ ] **Step 2: Delete Lua stdlib and type stubs**

Remove `prompts/stdlib/`, `assets/agents/.types/ghost.lua`.

- [ ] **Step 3: Delete Lua tooling config**

Remove `.luarc.json`, `.stylua.toml`.

- [ ] **Step 4: Remove mlua from Cargo.toml**

Delete the `mlua` line from `[dependencies]`.

- [ ] **Step 5: Delete lua-scripting skill**

Remove `.agents/skills/lua-scripting/SKILL.md`.

- [ ] **Step 6: Run full CI**

Run: `just ci`

All tests must pass with zero warnings. Fix any remaining references to Lua types or
modules.

- [ ] **Step 7: Commit**

```
chore: remove Lua runtime (mlua, scripting/, stdlib, tooling config)
```

### Task 14: Update flake.nix

**Files:**

- Modify: `assets/shell/flake.nix`

- [ ] **Step 1: Add rust-overlay input and Rust toolchain**

Add `rust-overlay` flake input. Add `rust-toolchain` (stable with `wasm32-wasip2`
target), `cargo-component`, and `wasm-tools` to the shell environment paths.

Reference: spec section "Flake changes" has the complete nix code.

- [ ] **Step 2: Verify nix build**

Run: `nix build .#` in the shell directory (or however the workspace shell is tested).

- [ ] **Step 3: Commit**

```
chore: add Rust WASM toolchain to workspace shell flake
```

### Task 15: Update agent-creator skill + docs references

**Files:**

- Modify: `assets/skills/agent-creator/skill.md`
- Modify: `CLAUDE.md` / `AGENTS.md`
- Modify: `README.md`
- Modify: all doc pages listed in spec migration steps 16-27

- [ ] **Step 1: Rewrite agent-creator skill**

Replace all Lua examples with Rust `agent!` macro examples. Update file layout diagrams.
Remove nudge library section. Update custom tools section to show `#[derive(Schema)]`
pattern. Update crontab section to show TOML format.

- [ ] **Step 2: Update CLAUDE.md / AGENTS.md**

Replace "Lua agent loading, scheduling, runner, watcher" with WASM equivalents. Update
project layout to show `agents/runtime.rs`, `agents/wasm_host.rs`, `agents/compiler.rs`
instead of `scripting/`. Remove Lua from the language/tooling mentions.

- [ ] **Step 3: Update README.md**

Replace "Lua agents" with "WASM agents" in feature descriptions.

- [ ] **Step 4: Update docs/ pages**

Go through each doc page listed in spec migration steps 16-27. Replace Lua syntax with
Rust SDK syntax. Replace `agent.lua` references with `agent.rs`. Replace `crontab.lua`
with `crontab.toml`. Remove any mention of the nudge library.

Full list:

- `docs/src/content/docs/agents/introduction.md`
- `docs/src/content/docs/agents/syntax.md`
- `docs/src/content/docs/agents/context.md`
- `docs/src/content/docs/agents/cron.md`
- `docs/src/content/docs/agents/agent-control.md`
- `docs/src/content/docs/knowledge/reflection.md`
- `docs/src/content/docs/chat/compaction.md`
- `docs/src/content/docs/disclaimer.md`
- `docs/src/content/docs/reference/cli.md`
- `docs/src/content/docs/ghost/providers.md`
- `docs/src/content/docs/skills-and-tools/default-skills.md`
- `docs/src/content/docs/user-guide.md`

- [ ] **Step 5: Grep for any remaining "lua" references**

Run: `rg -i "lua" --type md --type toml --type nix` across the repo. Fix any stragglers.

- [ ] **Step 6: Commit**

```
docs: update all documentation for WASM agent runtime
```

---

## Verification

After all tasks are complete:

- [ ] `just ci` passes with zero warnings
- [ ] `rg -i "lua" src/` returns zero results (excluding comments about the migration)
- [ ] `rg -i "mlua" Cargo.toml` returns zero results
- [ ] All 8 agents compile to WASM and pass `ghost agent validate`
- [ ] Crontab loads from TOML
- [ ] Scheduler triggers agents on idle/cron
- [ ] `ghost agent list` discovers agents from both `agents/` and `skills/`
- [ ] Agent spawning (deep-research → reflection) works end-to-end
