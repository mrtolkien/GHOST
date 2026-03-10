# Cron Agents: ctx:call_tool + Daily Recap + E2E Test

## Problem

Agents can't gather context in their Lua hooks (`build`, `should_trigger`, etc.) because
`ctx` only exposes DB/state methods. This means every agent must give the LLM tools to
fetch data, wasting tokens and iterations on mechanical fetching. Additionally, the
agent-creator skill doesn't cover scheduled/cron agents.

## Changes

### 1. `ctx:call_tool(name, args)` on AgentContext

New async method in `src/scripting/bindings.rs`. Lets any Lua hook call any tool.

- Routes through the tool execution layer (needs `Arc<ToolManager>` or equivalent on
  `AgentContext`)
- **Unrestricted**: not gated by the agent's `tools` whitelist. The whitelist controls
  what the LLM sees; `call_tool` is for the Lua author, a different trust boundary.
- Returns the tool result string on success, raises Lua error on failure
- Available in all hooks: `build`, `should_trigger`, `post_completion`, custom tool
  handlers

Example:
```lua
build = function(ctx, args)
    local feed = ctx:call_tool("web_fetch", { url = "https://example.com/feed.xml" })
    return {
        system_prompt = "Summarize this feed.",
        messages = {
            { role = "user", content = feed },
        },
    }
end
```

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

- Parallel `ctx:call_tools()` batch variant (can add later if needed)
- Bundling the daily-recap agent in `assets/` (it's a test fixture)
- Scheduler-level testing (already covered by unit tests in `crontab.rs`)
