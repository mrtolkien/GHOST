# `ghost run`, `run` tool, and `schedule.toml`

## Problem

Ghost has several systems for running code that evolved independently:

- **Agents** (Lua, in `agents/` and `skills/*/`): triggered by cron/idle via
  `crontab.lua`, or manually via the `agent` tool in chat. Have LLM loops, hooks, custom
  tools.
- **Scripts** (Python, in `skills/*/scripts/` and `scripts/`): run ad-hoc via shell tool
  (`uv run ...`). No lifecycle management, no background tracking.
- **`crontab.lua`**: Lua file defining schedules — code where config would suffice. Not
  observable through CLI (no `ghost schedule list`), not inspectable at a glance.

Agents and scripts are different things (agents have prompts, tools, hooks; scripts run
deterministically), but they converge in lifecycle: both are processes that take time,
need to be started, observed, and managed.

## Goal

Three changes:

1. **`ghost run`** — CLI command that runs both agents and skill scripts, with background
   job tracking (status, cancel, logs).
2. **`run` tool** — replaces the `agent` tool in chat. Same actions (start, status, show,
   cancel), but handles both agents and scripts. The GHOST uses one tool for all managed
   processes.
3. **`schedule.toml`** — replaces `crontab.lua`. Plain TOML at the workspace root.
   Observable via `ghost schedule list`. Editable by the GHOST as a text file.

## `nix run` for native dependencies (DONE)

Implemented in `2375fb4`. `pdftoppm` now runs via `nix run nixpkgs#poppler_utils`
instead of requiring `poppler_utils` in the permanent shell flake. Pattern for future
use: any rarely-used CLI tool should use `nix run` instead of being added to the flake.

## `ghost run` — unified CLI

### Interface

```
ghost run <target> [args...]             Run a script or agent
ghost run <target> --background [args..] Run in background
ghost run status [--limit N]             Show running/recent runs
ghost run show <run-id> [--full]         Show run details and output
ghost run cancel <run-id>                Cancel a running process
ghost run list                           List all runnable targets
```

### Target resolution

A target is `<scope>:<name>` where scope determines where to look:

```
ghost run deep-research:agent            # agents/deep-research/agent.lua
ghost run image-generation:generate      # skills/image-generation/scripts/generate.py
ghost run document-processing:convert    # skills/document-processing/scripts/convert.py
```

Resolution order for `<scope>:<name>`:
1. `$WORKSPACE/agents/<scope>/` — look for `<name>.lua`, `<name>.py`, `<name>.wasm`
2. `$WORKSPACE/skills/<scope>/scripts/` — same file search
3. Error if not found in either

### Dispatch by file type

| Extension | Runtime | How it runs |
|---|---|---|
| `.py` | Python/uv | `uv run <script.py> [args]` |
| `.lua` | Lua agent runner | Existing `AgentRunner::run()` path |
| `.wasm` | wasmtime | Future — WASM agents spec |

Ghost infers the runtime from the file. No manifest needed.

### Background execution and job lifecycle

Foreground (default): streams stdout/stderr, blocks until completion, Ctrl+C cancels.

Background (`--background`): returns a run ID immediately. Output captured to DB. Reuses
the existing `agent_runs` infrastructure, generalized to track both agent and script
runs.

- `ghost run status` — shows all runs (manual + scheduled), replaces `ghost agent status`
- `ghost run show <id>` — shows run output and details
- `ghost run cancel <id>` — sends cancellation signal

### Discovery

`ghost run list` scans:
1. `$WORKSPACE/agents/*/` — for `.lua`, `.py`, `.wasm` files
2. `$WORKSPACE/skills/*/scripts/` — same
3. Displays: `<scope>:<name> — <runtime>`

### Subsumes `ghost agent`

| Current | New |
|---|---|
| `ghost agent list` | `ghost run list` |
| `ghost agent validate <name>` | `ghost run validate <target>` |
| `ghost agent status` | `ghost run status` |
| `ghost agent show <id>` | `ghost run show <id>` |

`ghost agent` remains as a deprecated alias during transition.

## `run` tool — replaces `agent` tool in chat

The current `agent` tool has actions: `start`, `status`, `show`, `cancel`. The `run` tool
keeps the same actions but handles both agents and scripts:

```json
{
  "name": "run",
  "description": "Start and manage background processes (agents and scripts)",
  "parameters": {
    "action": "start | status | show | cancel",
    "target": "deep-research:agent",
    "prompt": "Research quantum computing",
    "run_id": "abc123"
  }
}
```

- `start` with a `.lua` target: runs as agent (prompt becomes the agent's input)
- `start` with a `.py` target: runs as script (prompt/args passed as CLI arguments)
- `status`, `show`, `cancel`: work identically for both — they operate on run IDs

The GHOST no longer needs to know whether something is an "agent" or a "script" at the
tool level. It just runs things and checks on them.

## `schedule.toml` — replaces `crontab.lua`

### Format

Single file at `$WORKSPACE/schedule.toml`:

```toml
[[schedule]]
target = "deep-research:agent"
trigger = "cron"
cron = "0 3 * * *"
description = "Nightly deep research sweep"

[[schedule]]
target = "chat-reflection:agent"
trigger = "idle"
idle_minutes = 30
description = "Reflect on conversation after 30 min silence"

[[schedule]]
target = "morning-briefing:agent"
trigger = "cron"
cron = "0 8 * * MON-FRI"
description = "Weekday morning briefing"
```

### Why one flat file

- **Observable**: `cat schedule.toml` or `ghost schedule list` shows everything at a
  glance. No hunting through nested directories.
- **Text-file philosophy**: the GHOST reads and edits it like any other config file.
- **Simple**: TOML, not Lua. `toml::from_str` replaces the Lua VM for parsing schedules.

### CLI

```
ghost schedule list                      Show all schedules with next/last run
ghost schedule enable <target>           Enable a schedule
ghost schedule disable <target>          Disable a schedule
ghost schedule history [--limit N]       Show recent scheduled runs
```

### Scheduler internals

The scheduler watches `schedule.toml` for changes (same debounced file watcher pattern as
the current `crontab.lua` watcher). On change, it reloads all entries.

Execution uses `ghost run` infrastructure internally — scheduled runs appear in
`ghost run status` alongside manual ones, tagged with their trigger type (cron/idle).

The `ScheduleEntry` struct:

```rust
struct ScheduleEntry {
    target: String,           // "deep-research:agent"
    trigger: ScheduleTrigger, // Cron { expr, next_fire } | Idle { minutes }
    description: Option<String>,
    enabled: bool,
}

enum ScheduleTrigger {
    Cron { cron: String },
    Idle { idle_minutes: u64 },
}
```

### Migration from `crontab.lua`

1. Parse existing `crontab.lua` entries
2. Generate equivalent `schedule.toml`
3. Switch scheduler to read TOML instead of Lua
4. Delete `crontab.lua` loading code (reduces mlua dependency surface)

## Migration path

### Phase 1: `ghost run` for scripts and agents

1. Add `ghost run` CLI subcommand with target resolution and dispatch
2. Wire Python scripts through `uv run`
3. Wire Lua agents through existing `AgentRunner`
4. Generalize `agent_runs` table for all run types
5. Add `ghost run status`, `ghost run show`, `ghost run cancel`
6. Add `ghost run list` with discovery across agents/ and skills/

### Phase 2: `run` tool replaces `agent` tool

1. Create `run` tool with same actions as `agent` tool
2. Wire `start` action to `ghost run` dispatch (both agents and scripts)
3. Deprecate `agent` tool (keep as alias initially)
4. Update system prompt to reference `run` tool

### Phase 3: `schedule.toml` replaces `crontab.lua`

1. Define `ScheduleEntry` types and TOML parsing
2. Migrate scheduler to read `schedule.toml` instead of `crontab.lua`
3. Auto-generate `schedule.toml` from existing `crontab.lua` on first boot
4. Wire scheduled runs through `ghost run` infrastructure
5. Add `ghost schedule list`, `enable`, `disable`, `history`
6. Remove Lua crontab loading code

### Phase 4: skill-level config migration

1. Move `[docling]` config from core config.toml to skill-level config.toml
2. Update scripts and Rust code to read from skill directory
3. Repeat for other extension-specific config sections as they arise

### Future: WASM runtime

Per the WASM agents spec (`backlog/tasks/5-management-safety/wasm-agents.md`).
`ghost run` gains `.wasm` dispatch. `agent!` and `script!` macros provide the authoring
experience.

## Testing

- **Bundled scripts**: smoke tests in `assets/skills/*/tests/`, run via
  `just test-scripts` in CI.
- **GHOST-authored scripts**: the GHOST runs them after writing. No test framework in the
  workspace — the development loop is write → run → check → fix.

## Documentation updates

When this ships:

- **CLAUDE.md / AGENTS.md**: add `ghost run` and `run` tool. Document `nix run` pattern.
- **Nix shell skill**: teach `nix run` as preferred method for one-off tool deps.
- **User-facing docs**: add `ghost run` and `ghost schedule` reference pages.
- **Skill authoring**: document that skills with scripts should include `ghost run`
  invocation examples in skill.md.

## Open questions

- **Agent discovery unification.** Agents live in `agents/`, skill scripts in
  `skills/*/scripts/`. Both are discoverable via `ghost run`. Long-term, should agents
  move into skills? Deferred — both paths work, no migration urgency.
- **Service definitions.** Should services (docker-compose fragments) also live in skill
  directories? Deferred.
