# Cron Agents Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if
> subagents available) or superpowers:executing-plans to implement this plan. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let Lua agent hooks call any tool via `ctx:call_tool()`, update the
agent-creator skill to cover scheduled agents, and write an e2e test that verifies GHOST
can create a cron agent from a chat message.

**Architecture:** Add `Config` + `Arc<ToolManager>` to `AgentContext` so Lua hooks can
execute tools. The `ToolManager` is constructed with `all_available()` (unrestricted).
A `ToolContext` is built on the fly from AgentContext fields.

**Tech Stack:** Rust (mlua async methods, serde_json), Lua agent definitions, live e2e
tests with `--features live-tests`.

---

## Chunk 1: `ctx:call_tool` Implementation

### Task 1: Extend AgentContext with tool execution capability

**Files:**
- Modify: `src/scripting/bindings.rs` (AgentContext struct + methods)
- Modify: `src/tools/manager.rs:82` (make `all_available()` public)
- Modify: `src/agents/runner.rs:460, 558, 630` (AgentContext construction — 3 sites)

- [ ] **Step 1: Add fields to AgentContext**

In `src/scripting/bindings.rs`, add `config` and `tool_manager` to `AgentContext`:

```rust
use crate::config::Config;
use crate::tools::manager::ToolManager;

pub struct AgentContext {
    pub db: GhostDb,
    pub workspace: PathBuf,
    pub agent_slug: String,
    pub session_id: String,
    pub trigger_session_id: Option<String>,
    pub spawn_requests: Arc<Mutex<Vec<SpawnRequest>>>,
    pub system_prompt: Arc<Mutex<Option<String>>>,
    pub resume_messages: Arc<Mutex<Option<Vec<LuaMessage>>>>,
    // New fields:
    pub config: Option<Config>,
    pub tool_manager: Option<Arc<ToolManager>>,
}
```

Use `Option` so existing call sites that don't need tool execution (tests, simple
contexts) don't break. Update `AgentContext::new()` to initialize both as `None`.

- [ ] **Step 2: Add a builder method for tool capability**

```rust
impl AgentContext {
    pub fn with_tool_support(mut self, config: Config, tool_manager: Arc<ToolManager>) -> Self {
        self.config = Some(config);
        self.tool_manager = Some(tool_manager);
        self
    }
}
```

- [ ] **Step 3: Add the `call_tool` async method**

In the `add_methods` block of `impl LuaUserData for AgentContext`:

mlua is compiled with the `"serde"` feature (check `Cargo.toml`), so use
`lua.from_value::<serde_json::Value>(args)` for Lua→JSON conversion. No manual
conversion helper needed.

```rust
// ctx:call_tool(name, args_table) -> string
methods.add_async_method(
    "call_tool",
    |lua, this, (name, args): (String, LuaValue)| async move {
        let config = this.config.as_ref().ok_or_else(|| {
            LuaError::external("call_tool not available: agent context has no tool support")
        })?;
        let tool_manager = this.tool_manager.as_ref().ok_or_else(|| {
            LuaError::external("call_tool not available: agent context has no tool manager")
        })?;

        // Convert Lua value to serde_json::Value via mlua's serde integration
        let params: serde_json::Value = lua.from_value(args)?;

        let tool_ctx = crate::tools::context::ToolContext {
            workspace: this.workspace.clone(),
            cwd: this.workspace.clone(),
            db: this.db.clone(),
            config: config.clone(),
            session_id: this.session_id.clone(),
            agent_runner: None,
            event_tx: None,
            channel_id: None,
        };

        let output = tool_manager
            .execute(&name, params, &tool_ctx)
            .await
            .map_err(|e| LuaError::external(format!("call_tool({name}) failed: {e}")))?;

        Ok(output.text)
    },
);
```

Note: the method takes `LuaValue` (not `LuaTable`) to handle all input types.

- [ ] **Step 4: Make `ToolManager::all_available()` public**

In `src/tools/manager.rs:82`, change `fn all_available()` to `pub fn all_available()`.
It's currently private but we need it from `src/agents/runner.rs`.

- [ ] **Step 5: Wire tool support into all AgentContext creation sites**

There are **three** places in `src/agents/runner.rs` that create `AgentContext` and all
need tool support:

**a) `setup_agent` (line 460)** — the `build()` hook context:
```rust
let hook_tool_manager = Arc::new(ToolManager::all_available());

let mut ctx = AgentContext::new(
    db.clone(),
    config.workspace.clone(),
    agent_name.to_string(),
    agent_session_id.to_string(),
);
ctx.trigger_session_id = parent_session_id.map(String::from);
ctx = ctx.with_tool_support(config.clone(), Arc::clone(&hook_tool_manager));
```

**b) `setup_resume` (line 558)** — the `on_resume` hook context:
```rust
let mut resume_ctx = AgentContext::new(...);
resume_ctx = resume_ctx.with_tool_support(config.clone(), Arc::new(ToolManager::all_available()));
```

**c) `run_post_completion` (line 630)** — the `post_completion` hook context:
```rust
let mut ctx = AgentContext::new(...);
ctx = ctx.with_tool_support(config.clone(), Arc::new(ToolManager::all_available()));
```

`run_post_completion` already receives `config: &Config`, so just add the tool manager.
The `Arc<ToolManager>` is cheap to construct (just registers tool structs).

- [ ] **Step 7: Run `just ci` to verify compilation**

Run: `just ci`
Expected: All checks pass. No existing tests break since new fields are `Option<_>`
defaulting to `None`.

- [ ] **Step 8: Commit**

```
feat: add ctx:call_tool() for Lua agent hooks
```

### Task 2: Unit test for `call_tool`

**Files:**
- Create: `tests/call_tool_unit.rs` (or add to existing agent test file)

- [ ] **Step 1: Write a test that calls `web_fetch` from a build hook**

This needs a small integration test — create a Lua agent whose `build()` calls
`ctx:call_tool("read_file", { path = "test.txt" })` and verify the content is returned
in the build result messages.

Use a tempdir workspace, write a file, load the agent, call `build()`, check the
message content includes the file contents.

Check the @testing skill for the right test harness patterns. This should be a unit
test in `src/scripting/bindings.rs` `#[cfg(test)]` module, or an integration test if it
needs full tool infra.

- [ ] **Step 2: Run the test**

Run: `cargo test call_tool -- --nocapture`
Expected: PASS

- [ ] **Step 3: Commit**

```
test: unit test for ctx:call_tool in build hook
```

## Chunk 2: Agent Creator Skill Update

### Task 3: Add scheduled agents section to agent-creator skill

**Files:**
- Modify: `assets/skills/agent-creator/skill.md`

- [ ] **Step 1: Add "Scheduled Agents" section**

After the existing content (which covers skill+agent pairs), add a new section. Key
content to cover:

1. Scheduled agents live in `agents/{name}/` (NOT in `skills/`)
2. Must add entry to `agents/crontab.lua`
3. Crontab format: Lua table, each entry has `cron` or `idle_minutes` + `run`
4. Cron expressions: standard 5-field (`minute hour day month dow`)
5. Show the `ctx:call_tool()` pattern for pre-fetching data in `build()`
6. Show `ctx:get/set` for cross-run state persistence
7. Emphasize: `tools = {}` when the LLM should only synthesize pre-fetched data
8. Show a complete example agent that fetches URLs and produces a digest

The section should include a full working example of `agent.lua` + `prompt.md` +
crontab entry for a daily digest agent, so GHOST has a concrete template to follow.

Example crontab.lua snippet:
```lua
return {
    { idle_minutes = 30, run = "chat-reflection" },
    { cron = "0 7 * * *", run = "daily-digest" },
}
```

Example agent.lua (complete):
```lua
local template = require("ghost.template")

return {
    name = "daily-digest",
    description = "Fetches RSS feeds and produces a daily recap",

    tools = {},          -- LLM has no tools, just synthesizes
    max_iterations = 1,  -- one turn only

    build = function(ctx, args)
        local urls = {
            "https://example.com/feed.xml",
            "https://other.com/news",
        }
        local fetched = {}
        for _, url in ipairs(urls) do
            local ok, result = pcall(function()
                return ctx:call_tool("web_fetch", { url = url })
            end)
            if ok then
                table.insert(fetched, "## " .. url .. "\n\n" .. result)
            else
                table.insert(fetched, "## " .. url .. "\n\n[fetch failed: " .. tostring(result) .. "]")
            end
        end

        local previous = ctx:get("last_digest") or "No previous digest."

        return {
            system_prompt = template.render(read_file("prompt.md"), {
                date = os.date("%Y-%m-%d"),
            }),
            messages = {
                { role = "user", content = "# Previous digest\n\n" .. previous
                    .. "\n\n# Today's feeds\n\n" .. table.concat(fetched, "\n\n") },
            },
        }
    end,

    post_completion = function(ctx)
        -- Save the digest so the next run can skip already-reported items.
        -- post_completion gets a fresh ctx with the agent's session_id.
        -- Use ctx:list_messages() to read back the agent's own output.
        local messages = ctx:list_messages(ctx.session_id)
        local last_msg = messages[#messages]
        if last_msg and last_msg.role == "assistant" then
            ctx:set("last_digest", last_msg.content)
        end
    end,
}
```

Note: `post_completion` does NOT receive the findings directly. It gets a fresh
`AgentContext` with the agent's `session_id`, so use `ctx:list_messages(ctx.session_id)`
to read back the agent's conversation and extract the last assistant message.

- [ ] **Step 2: Update the Lua type stubs**

In `assets/agents/.types/ghost.lua`, add the `call_tool` method signature so agent
authors get IDE completion:

```lua
---@async
---@param name string Tool name (e.g. "web_fetch", "read_file")
---@param args table Tool arguments as key-value pairs
---@return string result The tool's output text
function AgentContext:call_tool(name, args) end
```

- [ ] **Step 3: Verify the skill reads correctly**

Read the updated file and check it flows naturally from the existing spawn-agent
content into the new scheduled-agent content.

- [ ] **Step 4: Commit**

```
docs: add scheduled agents section to agent-creator skill
```

## Chunk 3: E2E Test

### Task 4: Write the daemon e2e test

**Files:**
- Create: `tests/daemon/cron_agent.rs`
- Modify: `tests/daemon.rs` (add module declaration)

Read the @testing and @e2e-testing skills before starting this task.

- [ ] **Step 1: Add module to daemon.rs**

```rust
#[path = "daemon/cron_agent.rs"]
mod cron_agent;
```

- [ ] **Step 2: Write the test skeleton**

```rust
use std::time::Duration;
use crate::helpers::live_test_database;

#[tokio::test]
async fn test_cron_agent_creation() {
    let env = live_test_database("cron_agent_creation").await;
    let daemon = env.boot_daemon().await;

    let session_id = ghost::db::sessions::create_session(&daemon.db)
        .await
        .expect("create session");

    // Step 1: Ask GHOST to create a daily recap agent
    let timeout = Duration::from_secs(300);
    tokio::time::timeout(timeout, async {
        daemon.session_chat
            .chat(
                &session_id,
                "Please make me a recap of what's new from these websites every day at 7AM:\n\
                 - https://all3dp.com/3d-printing-news/\n\
                 - https://www.dpreview.com/feeds/news.xml\n\
                 - http://www.gsmarena.com/rss-news-reviews.php3",
                None,
                None,
            )
            .await
            .expect("chat failed");
    })
    .await
    .expect("TIMEOUT: agent creation exceeded 300s");

    daemon.settle().await.expect("settle after creation");

    // Step 2: Assert agent files were created
    // The agent name may vary — search for any new agent dir
    let agents_dir = env.workspace_path().join("agents");
    let agent_dirs: Vec<_> = std::fs::read_dir(&agents_dir)
        .expect("read agents/")
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .filter(|e| {
            // Exclude known bundled agents
            let name = e.file_name();
            let name = name.to_string_lossy();
            name != "chat-reflection" && name != ".types"
        })
        .collect();

    assert!(
        !agent_dirs.is_empty(),
        "expected GHOST to create an agent directory under agents/"
    );

    let agent_dir = &agent_dirs[0];
    let agent_name = agent_dir.file_name().to_string_lossy().to_string();

    let agent_lua = agent_dir.path().join("agent.lua");
    assert!(agent_lua.exists(), "expected agents/{agent_name}/agent.lua");

    let prompt_md = agent_dir.path().join("prompt.md");
    assert!(prompt_md.exists(), "expected agents/{agent_name}/prompt.md");

    // Verify agent.lua references call_tool and the URLs
    let lua_content = std::fs::read_to_string(&agent_lua).expect("read agent.lua");
    assert!(
        lua_content.contains("call_tool"),
        "agent.lua should use ctx:call_tool for pre-fetching"
    );
    assert!(
        lua_content.contains("web_fetch"),
        "agent.lua should call web_fetch tool"
    );

    // Step 3: Verify crontab.lua has the new entry
    let crontab_entries = ghost::agents::crontab::load_crontab(env.workspace_path())
        .expect("parse crontab.lua");
    let has_new_entry = crontab_entries.iter().any(|e| e.run == agent_name);
    assert!(
        has_new_entry,
        "crontab.lua should contain entry for '{agent_name}', found: {:?}",
        crontab_entries.iter().map(|e| &e.run).collect::<Vec<_>>()
    );

    // Step 4: Run the created agent
    env.log("running created agent...");
    let agent_result = daemon
        .agent_runner
        .run(&agent_name, "Execute the scheduled agent.", Some(&session_id))
        .await
        .expect("agent run failed");

    let findings = &agent_result.findings;
    env.log(&format!("agent findings length: {}", findings.len()));

    // Step 5: Assert findings contain content from at least one feed
    // (all3dp might fail due to Cloudflare, but dpreview and gsmarena should work)
    let findings_lower = findings.to_lowercase();
    let has_feed_content = findings_lower.contains("dpreview")
        || findings_lower.contains("gsmarena")
        || findings_lower.contains("3d print")
        || findings_lower.contains("phone")
        || findings_lower.contains("camera");
    assert!(
        has_feed_content,
        "agent findings should reference content from the RSS feeds, got: {}",
        &findings[..findings.len().min(500)]
    );

    assert!(
        findings.len() > 200,
        "agent findings should be a substantive recap, got {} chars",
        findings.len()
    );

    // Log everything for inspection
    env.log_session_json("creation_chat", &session_id).await;

    daemon.shutdown().await;
}
```

- [ ] **Step 3: Verify the test compiles**

Run: `cargo test --features live-tests test_cron_agent_creation --no-run`
Expected: Compiles without errors.

- [ ] **Step 4: Run the test**

Run: `cargo test --features live-tests test_cron_agent_creation -- --nocapture`
Expected: PASS. Check `e2e-output/` for diagnostic artifacts.

This test will likely need iteration — the LLM may not create the exact file structure
on the first try. If it fails:
- Check `e2e-output/` diagnostic.json for the chat transcript
- Check what files GHOST actually created in the workspace snapshot
- Adjust assertions or the prompt if needed
- DO NOT weaken assertions — fix the skill or prompt instead

- [ ] **Step 5: Commit**

```
test: e2e test for cron agent creation from chat
```

### Task 5: Final verification

- [ ] **Step 1: Run `just ci`**

Run: `just ci`
Expected: All format, check, clippy, and test steps pass.

- [ ] **Step 2: Run the full live test suite**

Run: `cargo test --features live-tests -- --nocapture`
Expected: All live tests pass, including the new one.
