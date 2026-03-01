# Lua Scripting for Ghost Agents

## Context

Agent iteration is slow because workflow logic (nudges, post-processing, context assembly) is hardcoded in Rust. Reflection alone is 1100 lines of tightly coupled Rust. Changing "fork the session" to "start a new session" requires refactoring `run_fork_reflection` vs `run_inner`. Adding a `report_findings` tool for structured output requires new Rust tool implementations. This couples agent-specific behavior to the Rust compilation cycle.

**Goal**: Move agent workflow logic into Lua scripts. One folder per agent, one `agent.lua` file defining config + hooks + custom tools. Agents and jobs are unified — an agent with a `schedule` field IS a job.

**Decisions**: Lua via `mlua` (embedded, popular, LLM-friendly). Sandboxed (allowlist only). Big bang migration (no dual system). Stdlib embedded in binary. Agent state in DB (kill `.state/` files).

---

## Agent Folder Structure

```
$WORKSPACE/agents/
  deep-research/
    agent.lua      # config + hooks + custom tools
    prompt.md      # system prompt prose
  reflection/
    agent.lua
    prompt.md
  weekly-digest/   # was previously a "job"
    agent.lua
    prompt.md
```

## agent.lua Contract

```lua
local nudges = require("ghost.nudges")
local template = require("ghost.template")

return {
  -- Required
  name = "deep-research",
  description = "Iterative web research",

  -- Model config
  model = "fast",                    -- optional, nil = config default
  reasoning_effort = "high",         -- optional
  max_iterations = 30,               -- default: 50

  -- Trigger (replaces agent/job distinction)
  trigger = "dispatch",              -- "dispatch" | "schedule" | "after_idle" | "after_agent"
  schedule = "0 9 * * MON",         -- only for trigger=schedule
  idle_minutes = 30,                 -- only for trigger=after_idle

  -- Built-in tools (subset of Ghost's 10 tools)
  tools = { "web_search", "web_fetch", "todo", "read_file" },

  -- System prompt (default — overridden if build_context returns one)
  system_prompt = template.render(read_file("prompt.md"), {
    date = os.date("%Y-%m-%d"),
  }),

  -- Custom tools (Lua-defined, optional)
  custom_tools = {
    report_findings = {
      description = "Submit final report",
      parameters = { type = "object", properties = { ... }, required = { "report" } },
      handler = function(ctx, args) ... end,
      terminal = true,  -- ends agent run
    },
  },

  -- Hooks (all optional)
  build_context    = function(ctx) ... end,    -- returns {system_prompt?, user_message?}
  pre_turn         = nudges.compose(...),      -- (state) -> string|nil, injected after each tool iteration
  on_end_turn      = nudges.progress_gate(..), -- (state) -> string|nil, blocks EndTurn if non-nil
  post_completion  = function(ctx, result) end,-- post-processing
  should_trigger   = function(ctx) ... end,    -- gate for scheduled triggers
}
```

## Ctx Object (exposed to Lua hooks)

```
ctx:get(key)                    -> string|nil     -- agent_state DB table
ctx:set(key, value)             -- agent_state DB table
ctx.db:list_messages(sid)       -> [{role, content, tool_calls, created_at}]
ctx.db:count_messages_since(sid, rfc3339) -> number
ctx.db:find_note_by_title(t)   -> note|nil
ctx.db:create_cited_edge(nid, rid) -> string
ctx.db:list_interface_sessions() -> [session]
ctx.db:get_session(sid)         -> session
ctx.web_cache:classify(text, n) -> [classified]
ctx.web_cache:curate(classified) -> {moved, deleted}
ctx.web_cache:link_cited_edges(classified) -> number
ctx.web_cache:format_classified(classified) -> string
ctx:filter_transcript(messages) -> string
ctx:extract_agent_findings(msgs) -> string|nil
ctx:load_diary_today()          -> string|nil
ctx.session_id                  -- current session
ctx.agent_slug                  -- this agent's name
ctx.trigger_session_id          -- for after_agent/after_idle triggers
ctx.workspace                   -- workspace path
```

## Pre-turn State Object

```lua
state = {
  iteration, max_iterations, remaining,
  elapsed_seconds,
  tool_counts = { web_fetch = 7, ... },
  last_input_tokens, context_window,
  todo_summary = { total, completed, incomplete },
  temporal_fire_count,
  context_pressure_fired,  -- for one-shot nudges
}
```

## Embedded Stdlib

**`ghost.nudges`** — composable functions, each returns `(state) -> string|nil`:
- `nudges.compose(...)` — collect non-nil results, wrap in `<system-reminder>`
- `nudges.iteration_countdown({{remaining=10, message="..."}, ...})`
- `nudges.temporal({after_seconds, messages={"...", "..."}})`
- `nudges.tool_count({tool, min, message})`
- `nudges.recency({tool, window, message})`
- `nudges.context_pressure({threshold_pct, message})`
- `nudges.progress_gate({no_todo, incomplete})` — for `on_end_turn` hook
- All support `{remaining}`, `{minutes}`, `{incomplete}`, `{count}`, `{min}` interpolation

**`ghost.template`** — `template.render(text, {key=val})` replaces `{{key}}`

---

## Implementation Phases

### Phase 1: Lua Foundation

Add `mlua` and create the sandboxed runtime.

**Cargo.toml**: `mlua = { version = "0.10", features = ["lua54", "async", "serialize", "send"] }`

**Create**:
- `src/scripting/mod.rs` — barrel
- `src/scripting/host.rs` — `ScriptHost` struct:
  - `new(agent_dir, workspace)` — creates sandboxed Lua VM
  - `load_config()` — executes `agent.lua`, extracts returned table into Rust types
  - `lua()` — access inner VM for hook calls
- `src/scripting/types.rs` — `AgentConfig`, `LuaToolDef`, `PreTurnState`, conversion helpers

**Sandboxing** (in `host.rs`):
- Remove: `os.execute`, `os.remove`, `os.rename`, `os.exit`, `os.getenv`, `os.tmpname`, `os.setlocale`
- Remove: `io` (entire table), `loadfile`, `dofile` (globals), `package.loadlib`
- Keep safe: `os.date`, `os.time`, `os.clock`, `string`, `table`, `math`, `coroutine`, `type`, `pairs`, `ipairs`, `tostring`, `tonumber`, `select`, `error`, `pcall`, `xpcall`
- Redirect: `print()` → `logfire::debug!`

**Global host functions** (registered on VM):
- `read_file(path)` — relative to `agent_dir`, sandboxed within `workspace`
- `load_skill(name)` — reads `$WORKSPACE/skills/{name}/skill.md`, strips YAML frontmatter
- `json.encode(value)` / `json.decode(str)` — via mlua's serde support

**Stdlib loading**: `package.preload["ghost.nudges"]` and `package.preload["ghost.template"]` set to embedded `include_str!` content.

**Create stdlib files**:
- `prompts/stdlib/nudges.lua`
- `prompts/stdlib/template.lua`

**Verify**: Unit test loads minimal `agent.lua`, extracts config. Test sandbox blocks `os.execute`. Test `require("ghost.nudges")` loads. Test `read_file("prompt.md")` reads from agent dir.

---

### Phase 2: Agent Loader

Replace `src/agents/definition.rs` with Lua-based folder scanning.

**Create** `src/agents/loader.rs`:

```rust
pub struct AgentConfig {
    pub name: String,
    pub description: String,
    pub model: Option<String>,
    pub reasoning_effort: Option<ReasoningEffort>,
    pub max_iterations: usize,
    pub trigger: AgentTrigger,
    pub tools: Vec<String>,
    pub system_prompt: Option<String>,
    pub custom_tools: Vec<LuaToolDef>,
    // Hook presence flags (functions live in the Lua VM)
    pub has_build_context: bool,
    pub has_pre_turn: bool,
    pub has_on_end_turn: bool,
    pub has_post_completion: bool,
    pub has_should_trigger: bool,
}

pub enum AgentTrigger {
    Dispatch,
    Schedule { cron: String },
    AfterIdle { minutes: u64 },
    AfterAgent,
}
```

**Functions**:
- `discover_agents(workspace) -> Vec<AgentInfo>` — scan `agents/*/agent.lua`, extract name+description
- `load_agent(workspace, name) -> Result<AgentConfig>` — lightweight, drops VM after
- `load_agent_with_host(workspace, name) -> Result<(AgentConfig, ScriptHost)>` — keeps VM alive for hooks

**Embedded defaults**: Replace `DEFAULT_TASKS` with `DEFAULT_AGENTS`:
```rust
const DEFAULT_AGENTS: &[(&str, &[(&str, &str)])] = &[
    ("deep-research", &[("agent.lua", include_str!("...")), ("prompt.md", include_str!("..."))]),
    ("reflection", &[("agent.lua", include_str!("...")), ("prompt.md", include_str!("..."))]),
];
pub fn install_default_agents(workspace: &Path) -> io::Result<()>;
```

**Modify** `src/prompt/context.rs`: `build_ghost_agents()` calls `discover_agents()` instead of `discover_tasks()`.

**Verify**: Test discovers agents from Lua folders. Test `install_default_agents` creates folders correctly.

---

### Phase 3: Agent State DB

**Create** migration (next sequence number after existing):
```sql
CREATE TABLE agent_state (
    agent_slug TEXT NOT NULL,
    key TEXT NOT NULL,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (agent_slug, key)
);
```

**Create** `src/db/agent_state.rs`:
- `get_state(db, agent_slug, key) -> Result<Option<String>>`
- `set_state(db, agent_slug, key, value) -> Result<()>`
- `delete_state(db, agent_slug, key) -> Result<()>`

**Modify** `src/db/mod.rs`: add `pub mod agent_state;`

**Verify**: Round-trip unit test with in-memory SQLite.

---

### Phase 4: Ctx Bindings

Build the `ctx` Lua userdata exposed to hook functions.

**Create** `src/scripting/bindings.rs`:

```rust
pub struct AgentContext {
    pub db: GhostDb,
    pub workspace: PathBuf,
    pub agent_slug: String,
    pub session_id: String,
    pub trigger_session_id: Option<String>,
}
```

Registered as Lua userdata with methods for all `ctx.*` operations listed above.

**Async bridging**: Use `mlua`'s async function support (`lua.create_async_function()`). Hooks are called from the tool loop (already async), so async Lua functions integrate naturally. If issues arise, fallback to `tokio::task::spawn_blocking` around sync Lua calls.

**Web cache functions**: The helpers in `src/jobs/reflection.rs` (`classify_web_cache`, `curate_references`, `link_cited_edges`, `format_classified_cache`, `filter_transcript`, `extract_agent_findings`) need to be callable from bindings. They're already `pub` free functions — call them directly from the binding implementations. No need to move them yet (cleanup in Phase 8).

**Verify**: Unit test creating `AgentContext`, registering on Lua VM, calling `ctx:get()` / `ctx:set()`, `ctx.db:count_messages_since()`.

---

### Phase 5: Hook Integration

Wire Lua hooks into the agent execution pipeline. This is the core architectural change.

**Modify** `src/agents/runner.rs` — `run_task()`:
1. Replace `load_task()` → `load_agent_with_host()` (returns `AgentConfig` + `ScriptHost`)
2. If `config.has_build_context`: call `script_host.call_build_context(ctx)` to get system_prompt + user_message overrides
3. If no `build_context` hook: use `config.system_prompt` directly (already rendered by Lua at load time — no `{{ query }}` interpolation needed in Rust)
4. Build `ToolManager::for_agent(&config.tools)` + register custom tools (Phase 6)
5. Pass `ScriptHost` + `AgentConfig` to `SessionChat::chat_agent()`

**Modify** `src/chat/session.rs` — replace `TaskHandler` with `LuaTaskHandler`:

```rust
struct LuaTaskHandler<'a> {
    session_chat: &'a SessionChat,
    session_id: &'a str,
    system_prompt: String,
    config: &'a AgentConfig,
    script_host: &'a ScriptHost,
    ctx: AgentContext,
    // Runtime state
    started_at: Instant,
    iteration_count: usize,
    last_input_tokens: u32,
    temporal_fire_count: usize,
    event_tx: Option<&'a EventSender>,
    pending_todo_update: bool,
}
```

**`ToolLoopHandler` implementation**:
- `system_prompt()` → returns `self.system_prompt.clone()` (unchanged)
- `on_assistant_tool_use/on_tool_results/on_end_turn` → persist to DB (same as current)
- `post_tool_iteration(history, tokens)`:
  1. `apply_masking_if_needed(history)` (stays in Rust)
  2. TODO refresh injection (stays in Rust — framework concern)
  3. If `config.has_pre_turn`: build `PreTurnState`, call `script_host.call_pre_turn(state)`, inject result
  4. Increment `iteration_count`, update `temporal_fire_count` from state
- `check_progress_gate(history)`:
  1. If `config.has_on_end_turn`: build `PreTurnState`, call `script_host.call_on_end_turn(state)`
  2. If no hook: return `None` (allow EndTurn)

**Key `PreTurnState` fields from Rust**:
- `iteration`, `max_iterations`, `remaining` — from handler state
- `elapsed_seconds` — from `started_at.elapsed()`
- `tool_counts` — extracted from history (count tool names in assistant messages)
- `last_input_tokens`, `context_window` — from handler + session_chat
- `todo_summary` — from DB todo list query
- `temporal_fire_count` — tracked in handler, incremented when temporal nudge fires

**ScriptHost hook methods**:
- `call_pre_turn(&self, state: PreTurnState) -> Result<Option<String>>` — calls Lua `pre_turn(state)`
- `call_on_end_turn(&self, state: PreTurnState) -> Result<Option<String>>` — calls Lua `on_end_turn(state)`
- `call_build_context(&self, ctx: &AgentContext) -> Result<Option<BuildContextResult>>` — async
- `call_post_completion(&self, ctx: &AgentContext, result_text: &str) -> Result<()>` — async

**Verify**: Integration test with a Lua agent using `nudges.temporal({after_seconds=0, messages={"hurry"}})` — verify nudge appears in history. Test `on_end_turn` blocks when TODO is incomplete.

---

### Phase 6: Custom Tools

Lua-defined tools that the LLM can call, with `terminal` support.

**Create** `src/scripting/custom_tools.rs`:

```rust
pub struct LuaToolAdapter {
    tool_name: String,
    description: String,
    input_schema: Value,    // JSON Schema
    terminal: bool,
    handler_key: LuaRegistryKey,
    script_host: Arc<ScriptHost>,  // shared ref to the Lua VM
}

impl Tool for LuaToolAdapter {
    fn name(&self) -> &str { &self.tool_name }
    fn schema(&self) -> ToolDefinition { ... }
    async fn execute(&self, params: Value, _ctx: &ToolContext) -> Result<String, ToolError> {
        // Call Lua handler with (ctx, args) where args is params converted to Lua table
    }
}
```

**Modify** `src/tools/manager.rs`:
- Add `is_terminal(&self, tool_name: &str) -> bool` method — looks up tool, returns `false` for built-in tools
- Trait change: add `fn is_terminal(&self) -> bool { false }` default to `Tool` trait
- `LuaToolAdapter` overrides with `self.terminal`

**Modify** `src/chat/tool_loop.rs` — in the `StopReason::ToolUse` arm, after `execute_tool_calls`:
```rust
// Check for terminal tool calls
let any_terminal = tool_uses.iter().any(|call| {
    let name = call.get("name").and_then(Value::as_str).unwrap_or("");
    session_chat.tool_manager().is_terminal(name)
});
if any_terminal {
    // Extract the terminal tool's result as the agent's output
    let result = handler.on_end_turn(
        extract_terminal_result(&tool_results),
        StopReason::EndTurn, &[], None
    ).await?;
    metadata.iterations = iterations;
    metadata.duration = started_at.elapsed();
    return Ok((result, metadata));
}
```

**Modify** `src/agents/runner.rs` — when building `ToolManager`, register `LuaToolAdapter` instances from `config.custom_tools` alongside built-in tools.

**Verify**: Test agent with a custom terminal tool — verify handler is called, tool loop exits, result is returned as agent output.

---

### Phase 7: Unified Scheduling

Merge the job scheduler + reflection idle watcher into one agent scheduler.

**Rewrite** `src/jobs/scheduler.rs` → `src/agents/scheduler.rs`:
- Scans `$WORKSPACE/agents/` for folders (using `discover_agents` + `load_agent` for trigger info)
- For `trigger=schedule`: manages cron next_run, calls `should_trigger()` hook if present, then runs agent via `task_runner.run_to_completion()`
- For `trigger=after_idle`: checks interface sessions for idle time > `idle_minutes`, calls `should_trigger()`, then runs agent
- File watcher on `$WORKSPACE/agents/` — reload on folder changes
- Tick interval: `config.timing.scheduler_tick_seconds` (same as current)

**Modify** `src/agents/watcher.rs` — replace hardcoded reflection trigger:
```rust
// OLD: if !status.agent_name.contains("reflection") { reflection.run_after_agent_handoff(...) }
// NEW: find agents with trigger=after_agent, run each
let after_agent_agents = discover_agents_by_trigger(workspace, AgentTrigger::AfterAgent);
for agent_info in after_agent_agents {
    // Skip self-triggering
    if agent_info.name == status.agent_name { continue; }
    let (config, host) = load_agent_with_host(workspace, &agent_info.name)?;
    // Check should_trigger if present
    // Run via task_runner
}
```

The watcher no longer needs `Arc<ReflectionManager>` — it uses `TaskRunner` + `load_agent_with_host` directly.

**Modify** `src/daemon/run.rs`:
- Remove `ReflectionManager` creation
- Replace `crate::jobs::spawn_scheduler(...)` with `crate::agents::scheduler::spawn(...)`
- Remove `reflection.spawn_idle_watcher(...)` (idle watching is now in the unified scheduler)
- The agent watcher no longer takes `Arc<ReflectionManager>` — just `Arc<TaskRunner>` + config

**Fork vs new session**: For `trigger=after_agent`, the agent config can include `continue_trigger_session = true` to fork the triggering agent's session (current reflection behavior) or `false` (default) for a fresh session. The scheduler/watcher checks this flag and calls `task_runner.continue_to_completion()` vs `run_to_completion()`.

**Verify**: Test agent with `trigger=schedule` fires on cron. Test `trigger=after_idle` fires after idle. Test `should_trigger` returning `false` prevents execution.

---

### Phase 8: Port Agents + Cleanup

**Write Lua agents** (embedded in binary via `include_str!`):

`prompts/agents/deep-research/agent.lua` — port from current `prompts/agents/deep-research.md`:
- Tools, max_iterations, reasoning_effort from YAML → Lua table
- System prompt body → `prompt.md` read via `read_file("prompt.md")`
- Nudges (progress rules, temporal, context_pressure, progress_gate) → `pre_turn` + `on_end_turn` using `ghost.nudges`
- Optional: add `report_findings` custom terminal tool

`prompts/agents/deep-research/prompt.md` — system prompt markdown (body of current .md file)

`prompts/agents/reflection/agent.lua` — port from reflection.rs logic:
- `trigger = "after_idle"` with `idle_minutes` from config
- `should_trigger` — check message count since last run (replaces .state file check)
- `build_context` — assemble user message from transcript, diary, web cache (replaces `build_user_message`)
- `post_completion` — curate references, link edges, save handoff (replaces Rust post-processing)

`prompts/agents/reflection/prompt.md` — system prompt (body of current chat-reflection.md)

`prompts/agents/fork-reflection/agent.lua` — the after-agent path:
- `trigger = "after_agent"`, `continue_trigger_session = true`
- `build_context` — builds fork reflection prompt with note-writer skill
- `post_completion` — web cache curation
- `should_trigger` — skip if completed agent is itself a reflection

**Port existing jobs** — any `.md` files in `$WORKSPACE/jobs/` become `agents/*/agent.lua` folders with `trigger = "schedule"`.

**Modify** `src/config_workspace.rs`:
- `bootstrap_workspace`: call `install_default_agents()` instead of `install_default_tasks()`
- Keep creating `agents/` dir (was already there), remove `jobs/` dir creation
- Remove `.state/` dir creation (agent_state DB replaces it)

**Remove files**:
- `src/agents/definition.rs` — old YAML parser
- `src/agents/nudges.rs` — Rust nudge config types
- `src/jobs/definition.rs` — old job parser
- `src/jobs/scheduler.rs` — old job scheduler (replaced by `agents/scheduler.rs`)
- `src/jobs/reflection.rs` — gutted; free functions used by ctx bindings stay (move to `src/web/cache.rs` or keep as utils)
- `prompts/agents/deep-research.md` — replaced by folder
- `prompts/agents/chat-reflection.md` — replaced by folder

**Update module structure**:
- `src/agents/mod.rs` — remove `definition`, `nudges`; add `loader`, `scheduler`
- `src/jobs/mod.rs` — remove `definition`, `scheduler`; keep `mod.rs` as barrel for remaining job-related utilities if any; or remove entirely if empty
- `src/lib.rs` — add `pub mod scripting;`

**Verify**: `just ci` passes. `ghost agent list` shows both agents. Manual test: `ghost agent run deep-research "test query"` executes correctly.

---

## Dependency Graph

```
Phase 1 (Foundation) ─┬─> Phase 2 (Loader) ─┬─> Phase 5 (Hooks) ──> Phase 6 (Custom Tools)
                       │                      │
Phase 3 (State) ──────┴─> Phase 4 (Bindings) ┘        Phase 7 (Scheduling) ── needs Phase 2
                                                        Phase 8 (Port+Cleanup) ── needs ALL
```

Phases 1+3 can run in parallel. Phase 7 can start after Phase 2 (doesn't need hooks).

---

## Risk Mitigations

| Risk | Mitigation |
|---|---|
| mlua async integration | Write spike test in Phase 1. Fallback: `spawn_blocking` around sync Lua calls |
| Lua VM lifetime | VM lives in `LuaTaskHandler`, which lives for the duration of `run_tool_loop()` — naturally scoped |
| Thread safety | `mlua` `send` feature makes `Lua` Send+Sync. Tool calls are sequential within one agent |
| Error messages | `ScriptError` wraps `mlua::Error` with agent name + file path for clear debugging |
| Binary size | Lua54 adds ~300KB. Acceptable |
| Reflection complexity | Keep Rust helper functions (classify, curate, link_cited_edges), call them via ctx bindings. No need to rewrite in Lua |

---

## Critical Files Reference

| File | Role in migration |
|---|---|
| `src/chat/session.rs` | `TaskHandler` → `LuaTaskHandler` (Phase 5, most complex change) |
| `src/agents/runner.rs` | `run_task()` uses `ScriptHost` for lifecycle (Phase 5) |
| `src/chat/tool_loop.rs` | Terminal tool detection (Phase 6, ~10 lines added) |
| `src/tools/manager.rs` | `is_terminal()` + custom tool registration (Phase 6) |
| `src/agents/definition.rs` | Removed (Phase 8, replaced by `loader.rs`) |
| `src/jobs/reflection.rs` | Free functions reused by ctx bindings; manager removed (Phase 8) |
| `src/jobs/scheduler.rs` | Removed (Phase 8, replaced by `agents/scheduler.rs`) |
| `src/daemon/run.rs` | Rewired to unified scheduler, no ReflectionManager (Phase 7) |
| `src/config_workspace.rs` | Bootstrap agent folders instead of .md files (Phase 8) |
