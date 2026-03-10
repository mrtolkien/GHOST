# Cron Agents: ctx:call_tool + Daily Recap + E2E Test

## Problem

Agents can't gather context in their Lua hooks (`build`, `should_trigger`, etc.) because
`ctx` only exposes DB/state methods. This means every agent must give the LLM tools to
fetch data, wasting tokens and iterations on mechanical fetching. Additionally, the
agent-creator skill doesn't cover scheduled/cron agents.

## Changes

### 1. Tool execution from Lua hooks

Two new async methods on `AgentContext` in `src/scripting/bindings.rs`. Both route
through the tool execution layer (`Arc<ToolManager>` on `AgentContext`).

- **Unrestricted**: not gated by the agent's `tools` whitelist. The whitelist controls
  what the LLM sees; hook-level tool calls are for the Lua author, a different trust
  boundary.
- Available in all hooks: `build`, `should_trigger`, `post_completion`, custom tool
  handlers.

#### `ctx:call_tool(name, args) -> string`

Simple single-tool call. Returns the tool result text on success, raises Lua error on
failure. Use for one-off calls in `should_trigger`, `post_completion`, etc.

```lua
should_trigger = function(ctx)
    local status = ctx:call_tool("read_file", { path = "status.json" })
    return status:find("needs_update")
end
```

#### `ctx:call_tools(list) -> messages`

Batch call that returns **pre-formatted messages** ready to splice into `build()`'s
return value. The agent author never touches tool call IDs or message structure.

```lua
build = function(ctx, args)
    local previous = ctx:get("last_digest") or "No previous digest."

    -- Returns ready-to-use messages:
    -- [1] = { role = "assistant", tool_calls = [{id, name, input}, ...] }
    -- [2] = { role = "user", tool_results = [{tool_use_id, content}, ...] }
    local tool_msgs = ctx:call_tools({
        { "web_fetch", { url = url1 } },
        { "web_fetch", { url = url2 } },
        { "web_fetch", { url = url3 } },
    })

    table.insert(tool_msgs, {
        role = "user",
        content = "Previous digest:\n" .. previous .. "\n\nSummarize these feeds.",
    })

    return {
        system_prompt = template.render(read_file("prompt.md"), {
            date = os.date("%Y-%m-%d"),
        }),
        messages = tool_msgs,
    }
end
```

Under the hood, `call_tools`:
1. Generates unique IDs (e.g. `"build_1"`, `"build_2"`, ...) for each call
2. Executes tools sequentially (latency is not a concern for build hooks)
3. Returns two Lua table messages: one `assistant` message with all `tool_calls`, one
   `user` message with all `tool_results` (failed calls get `is_error = true`)

### 1b. Extend BuildMessage to support tool call format

`BuildMessage` (`src/scripting/types.rs`) currently only has `{role, content}`. Extend
it with optional `tool_calls` and `tool_results`:

```rust
pub struct BuildMessage {
    pub role: String,
    pub content: String,
    pub tool_calls: Option<Vec<serde_json::Value>>,
    pub tool_results: Option<Vec<serde_json::Value>>,
}
```

The Lua parsing of `build()` return value must extract these optional fields from
message tables (they're already Lua tables from `call_tools`, so just read them through).

**Persistence** — update `run_agent` in `session.rs` (~line 978) to use
`create_message_with_metadata` instead of `create_message` when tool_calls or
tool_results are present. The existing `convert_stored_message_to_provider_message`
in `convert.rs` already reconstructs `ToolUse`/`ToolResult` ContentBlocks from the
JSON columns, so provider history will be correct automatically.

**Provider history building** — update the history construction (~line 984) to build
proper `ContentBlock::ToolUse` and `ContentBlock::ToolResult` blocks instead of only
`ContentBlock::Text`. Or: just persist to DB and let `load_provider_history` handle it
(simpler — persist first, then load, instead of building history inline).

### 2. Update agent-creator skill

Add a "Scheduled Agents" section to `assets/skills/agent-creator/skill.md`:

- Scheduled agents live in `agents/{name}/` (not `skills/`)
- Must add an entry to `agents/crontab.lua` with `cron` or `idle_minutes`
- Cron format: standard 5-field (`minute hour day month dow`)
- Show `ctx:call_tool()` for pre-fetching data in `build()` so the LLM gets one
  efficient call with all context pre-loaded
- Show `ctx:get/set` for cross-run state to avoid repeating content across runs

### 3. Test fixture: daily-recap agent

Hand-written agent in the test workspace (not bundled). Validates the pattern works
before the e2e test verifies GHOST can create one.

**`agents/daily-recap/agent.lua`:**
- `build()` calls `ctx:call_tool("web_fetch", ...)` for each URL
- Loads `ctx:get("last_digest")` to know what was already reported
- Injects all fetched content + prior digest into messages
- `tools = {}` — LLM has no tools, just synthesizes and responds
- `max_iterations = 1`
- `post_completion` saves digest summary via `ctx:set("last_digest", ...)`

**`agents/daily-recap/prompt.md`:**
- "Summarize the following feeds into a concise daily recap"
- Skip items covered in the previous digest

**Crontab entry:** `{ cron = "0 7 * * *", run = "daily-recap" }`

**URLs:**
- `https://all3dp.com/3d-printing-news/` (Cloudflare-protected, tests crawl4ai path)
- `https://www.dpreview.com/feeds/news.xml` (RSS 2.0)
- `http://www.gsmarena.com/rss-news-reviews.php3` (RSS 2.0)

### 4. E2E daemon test

New test module `tests/daemon/cron_agent.rs`, added to `tests/daemon.rs`.

Test flow:
1. Boot daemon
2. Create session, send: "Please make me a recap of what's new from these websites
   every day: [3 URLs]"
3. GHOST reads agent-creator skill, creates agent files + crontab entry
4. Assert: `agents/{name}/agent.lua` exists with `call_tool("web_fetch", ...)`
5. Assert: `agents/{name}/prompt.md` exists
6. Assert: crontab.lua has a new entry (parse with `load_crontab()`)
7. Run the created agent via `agent_runner.run()`
8. Assert: agent produces findings with content referencing the RSS feeds

## Non-goals

- Parallel execution inside `call_tools` (sequential is fine for build hooks)
- Bundling the daily-recap agent in `assets/` (it's a test fixture)
- Scheduler-level testing (already covered by unit tests in `crontab.rs`)
