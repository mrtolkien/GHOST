---
name: lua-scripting
description: >-
  Ghost's Lua scripting layer internals. MUST READ before modifying anything in
  src/scripting/, prompts/stdlib/, prompts/agents/*/agent.lua, or agent hook logic.
  Covers: ScriptHost VM lifecycle, sandbox, host globals, ctx bindings, nudge library,
  template module, custom tools, type stubs, and the PreTurnState contract.
---

# Lua Scripting Layer

Ghost agents are configured via `agent.lua` files executed in a sandboxed Lua 5.4 VM
(mlua). This skill documents the **Rust ↔ Lua boundary** for developers working on the
scripting layer.

## Architecture

```
src/scripting/
├── mod.rs        # barrel — re-exports ScriptHost + types
├── host.rs       # ScriptHost: VM lifecycle, sandbox, hook execution
├── bindings.rs   # AgentContext (ctx userdata) — DB queries, spawn, resume
├── types.rs      # AgentConfig, PreTurnState, BuildResult, NudgeResult, LuaToolDef
└── custom_tools.rs  # Custom tool dispatch (Lua handler → JSON result)

prompts/
├── stdlib/
│   ├── nudges.lua   # ghost.nudges module (composable pre_turn/on_end_turn hooks)
│   └── template.lua # ghost.template module (mustache-style {{var}} rendering)
├── types/
│   └── ghost.lua    # LuaLS type stubs (installed to agents/.types/)
└── agents/*/agent.lua  # Agent definitions
```

## ScriptHost Lifecycle

1. `ScriptHost::new(agent_dir, workspace)` — creates Lua VM, sandboxes it, registers
   stdlib modules and host globals (`read_file`, `load_skill`, `json`, `print`)
2. `load_config()` — executes `agent.lua`, extracts the returned table into
   `AgentConfig`
3. Hook calls: `call_build()`, `call_pre_turn()`, `call_on_end_turn()`,
   `call_post_completion()`, `call_on_resume()` — invoke Lua functions via registry keys
4. `call_custom_tool()` — dispatches a tool call to the Lua handler function

The VM is dropped when the `ScriptHost` is dropped. For `load_agent()` (config-only),
the VM is immediately discarded. For `load_agent_with_host()`, it lives for the agent
session's duration.

## Sandbox

`sandbox_lua()` in `host.rs` removes dangerous globals:

- Removed: `os.execute`, `os.remove`, `os.rename`, `os.tmpname`, `os.exit`, `io`,
  `loadfile`, `dofile`, `debug`
- Kept: `os.date`, `os.time`, `os.clock`, `os.difftime`
- `require()` is replaced with a custom loader that only resolves `ghost.*` modules

## Host Globals

Injected by `ScriptHost::new()`, available in all agent.lua files:

| Global             | Signature         | Description                                             |
| ------------------ | ----------------- | ------------------------------------------------------- |
| `read_file(path)`  | `string → string` | Read file relative to agent dir, sandboxed to workspace |
| `load_skill(name)` | `string → string` | Read `skills/{name}/skill.md`, strips YAML frontmatter  |
| `json.encode(val)` | `any → string`    | Serialize Lua value to JSON                             |
| `json.decode(str)` | `string → any`    | Parse JSON string to Lua value                          |
| `print(...)`       | `...any → ()`     | Prints to stdout (sandboxed)                            |

## ctx Bindings (AgentContext userdata)

Registered via `register_ctx()` when an agent has a database connection. Available in
`build()`, `post_completion()`, `on_resume()`, and custom tool handlers.

### Fields (get/set)

| Field                    | Type         | Notes                         |
| ------------------------ | ------------ | ----------------------------- |
| `ctx.session_id`         | `string`     | Read-only                     |
| `ctx.agent_slug`         | `string`     | Read-only                     |
| `ctx.trigger_session_id` | `string?`    | Read-only                     |
| `ctx.workspace`          | `string`     | Read-only                     |
| `ctx.system_prompt`      | `string?`    | Read/write — `on_resume` hook |
| `ctx.messages`           | `Message[]?` | Read/write — `on_resume` hook |

### Async Methods

| Method                                   | Signature                           | Description                                             |
| ---------------------------------------- | ----------------------------------- | ------------------------------------------------------- |
| `ctx:get(key)`                           | `→ string?`                         | Get agent state KV                                      |
| `ctx:set(key, value)`                    | `→ ()`                              | Set agent state KV                                      |
| `ctx:delete(key)`                        | `→ ()`                              | Delete agent state KV                                   |
| `ctx:count_messages_since(sid, rfc3339)` | `→ integer`                         | Count messages after timestamp                          |
| `ctx:list_interface_sessions()`          | `→ {interface, session_id}[]`       | All interface sessions                                  |
| `ctx:filter_transcript(sid)`             | `→ string`                          | Filtered transcript for session                         |
| `ctx:list_messages(sid)`                 | `→ {role, content, created_at}[]`   | All messages in session                                 |
| `ctx:curate_web_cache(sid?)`             | `string? → {moved, deleted, edges}` | Classify + move web cache (defaults to current session) |

### Sync Methods

| Method                        | Signature   | Description             |
| ----------------------------- | ----------- | ----------------------- |
| `ctx:load_diary_today()`      | `→ string?` | Today's diary entry     |
| `ctx:spawn_agent(name, args)` | `→ ()`      | Queue child agent spawn |

## AgentConfig Extraction

`load_config()` reads the table returned by `agent.lua` and maps it to `AgentConfig`:

- `name`, `description` — required strings
- `model` — optional string (nil = default)
- `reasoning_effort` — optional "low"/"medium"/"high"
- `max_iterations` — integer, default 50
- `tools` — string array of built-in tool names
- `skills` — string array of skill names to inject
- `custom_tools` — array of tool definition tables (see custom_tools.rs)
- Hook presence: `has_build`, `has_pre_turn`, `has_on_end_turn`, `has_post_completion`,
  `has_on_resume` — set by checking if the function exists
- `compaction` — optional `AgentCompactionOverrides` table (fields: `threshold`,
  `keep_window`, `mask_preview_chars`, `instructions`)

Hook functions are stored as Lua registry keys, not extracted as values.

## PreTurnState Contract

The `PreTurnState` struct is converted to a Lua table via `IntoLua` (types.rs) and
passed to `pre_turn(state)` and `on_end_turn(state)`:

```lua
{
    iteration = 5,          -- current iteration (1-based)
    max_iterations = 30,    -- configured maximum
    remaining = 25,         -- max_iterations - iteration
    elapsed_seconds = 120,  -- wall clock since agent start
    tool_counts = { web_search = 3, file_read = 7 },
    last_input_tokens = 5000,
    context_window = 128000,
    todo_summary = { total = 5, completed = 2, incomplete = 3 },  -- or nil
    todo_text = "- [x] Step 1\n- [ ] Step 2",                     -- or nil
    temporal_fire_count = 0,
    context_pressure_fired = false,
}
```

## NudgeResult Contract

`call_pre_turn()` expects the Lua function to return either:

- `nil` — no nudge
- A string — nudge text only
- A table `{ text, temporal_fired, context_pressure_fired }` — full result

The Rust side maps this to `NudgeResult` (types.rs).

## Custom Tools

Defined in `custom_tools` array in agent.lua:

```lua
custom_tools = {
    {
        name = "my_tool",
        description = "What it does",
        parameters = {
            { name = "input", type = "string", description = "The input", required = true },
        },
        terminal = false,  -- if true, tool result ends the session
        handler = function(ctx, args)
            return "tool result string"
        end,
    },
}
```

Handler functions are stored as registry keys. `call_custom_tool()` looks up the key by
index, converts JSON args to a Lua table, calls the handler, and returns the string
result.

## Stdlib Modules

### ghost.nudges

Preloaded from `prompts/stdlib/nudges.lua`. Provides composable nudge factories:

- `compose(...)` — combine multiple nudges, returns a function
- `iteration_countdown(thresholds)` — warn at iteration milestones
- `temporal(config)` — warn after elapsed time, supports escalating messages
- `context_pressure(config)` — warn when context window usage exceeds threshold
- `progress_gate(config)` — reject end-turn if TODO items incomplete
- `todo_list()` — inject current TODO state
- `tool_count(config)` — warn based on tool usage counts
- `recency(config)` — warn based on recent tool activity

### ghost.template

Preloaded from `prompts/stdlib/template.lua`. Provides:

- `render(text, vars)` — replace `{{key}}` with values from vars table

## Type Stubs

LuaLS type annotations live in `prompts/types/ghost.lua` (embedded in binary, installed
to `agents/.types/ghost.lua`). These provide autocompletion for agent developers.

Update the stubs when modifying any host global or ctx binding.

## Key Patterns

- **Registry keys over closures**: Hook functions are stored as `LuaRegistryKey` in the
  VM, not as Rust closures. This avoids lifetime issues with mlua.
- **handler_key_index**: Custom tool handlers use a usize index into
  `ScriptHost.tool_handler_keys` so `LuaToolDef` stays `Send+Sync`.
- **Arc<Mutex<>> for shared state**: `spawn_requests`, `system_prompt`,
  `resume_messages` use Arc<Mutex<>> so ctx can be cloned across async boundaries.
