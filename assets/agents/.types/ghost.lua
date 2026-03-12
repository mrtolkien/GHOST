---@meta
--- Ghost Lua API type stubs for LuaLS.
--- Installed to $WORKSPACE/agents/.types/ by the binary.

-- ============================================================================
-- Host globals (injected by ScriptHost::new in src/scripting/host.rs)
-- ============================================================================

--- Read a file relative to the agent directory. Path is sandboxed to workspace.
---@param path string  Relative path from the agent directory
---@return string content  File contents
function read_file(path) end

--- Load a skill file from $WORKSPACE/skills/{name}/skill.md.
--- YAML frontmatter is automatically stripped.
---@param name string  Skill directory name
---@return string content  Skill body (markdown)
function load_skill(name) end

--- Print to stdout (sandboxed).
---@param ... any
function print(...) end

-- ============================================================================
-- json module
-- ============================================================================

---@class ghost.json
json = {}

--- Encode a Lua value to a JSON string.
---@param value any
---@return string
function json.encode(value) end

--- Decode a JSON string to a Lua value.
---@param str string
---@return any
function json.decode(str) end

-- ============================================================================
-- AgentContext (ctx userdata, registered via register_ctx)
-- ============================================================================

---@class AgentContext
---@field session_id string  Current agent session ID (read-only)
---@field agent_slug string  Agent name/slug (read-only)
---@field trigger_session_id? string  Session that triggered this agent (read-only)
---@field workspace string  Workspace directory path (read-only)
---@field system_prompt? string  Editable system prompt (on_resume hook, read/write)
---@field messages? LuaMessage[]  Editable message history (on_resume hook, read/write)
local ctx = {}

---@class LuaMessage
---@field role string
---@field content string

--- Get a persistent key-value state entry for this agent.
---@param key string
---@return string|nil
---@async
function ctx:get(key) end

--- Set a persistent key-value state entry for this agent.
---@param key string
---@param value string
---@async
function ctx:set(key, value) end

--- Delete a persistent key-value state entry.
---@param key string
---@async
function ctx:delete(key) end

--- Count messages in a session since a given timestamp.
---@param session_id string
---@param since string  RFC 3339 datetime string
---@return integer
---@async
function ctx:count_messages_since(session_id, since) end

--- List all interface sessions.
---@return InterfaceSession[]
---@async
function ctx:list_interface_sessions() end

---@class InterfaceSession
---@field interface string
---@field session_id string

--- Get filtered transcript for a session (assistant content only).
---@param session_id string
---@return string
---@async
function ctx:filter_transcript(session_id) end

--- Load today's diary entry from the workspace.
---@return string|nil
function ctx:load_diary_today() end

--- List all messages in a session.
---@param session_id string
---@return SessionMessage[]
---@async
function ctx:list_messages(session_id) end

---@class SessionMessage
---@field role string
---@field content string
---@field created_at string

--- Classify and curate web cache for the current session.
---@return CurationResult
---@async
function ctx:curate_web_cache() end

---@class CurationResult
---@field moved integer
---@field deleted integer
---@field edges integer

--- Execute a single tool and return its output text.
--- Only available when AgentContext has tool support (build, on_resume, post_completion).
---@param name string  Tool name (e.g. "web_fetch", "read_file")
---@param args table  Tool arguments as key-value pairs
---@return string result  The tool's output text
---@async
function ctx:call_tool(name, args) end

--- Execute multiple tools sequentially and return pre-formatted messages.
--- Returns two messages: [1] assistant with tool_calls, [2] user with tool_results.
--- Splice these directly into a build() return's messages list.
---@param calls table  List of {name, args} pairs, e.g. {{"web_fetch", {url = "..."}}}
---@return BuildMessage[] messages  Two pre-formatted messages (assistant + user)
---@async
function ctx:call_tools(calls) end

--- Spawn a child agent (queued, executed after post_completion returns).
---@param name string  Agent name to spawn
---@param args table<string, string>  Arguments passed to the child's build(ctx, args)
function ctx:spawn_agent(name, args) end

-- ============================================================================
-- PreTurnState (passed to pre_turn and on_end_turn hooks)
-- ============================================================================

---@class PreTurnState
---@field iteration integer  Current iteration (1-based)
---@field max_iterations integer  Configured maximum
---@field remaining integer  max_iterations - iteration
---@field elapsed_seconds integer  Wall clock since agent start
---@field tool_counts table<string, integer>  Tool name -> call count
---@field last_input_tokens integer  Input tokens in most recent turn
---@field context_window integer  Total context window size
---@field todo_summary? TodoSummary  TODO state if available
---@field todo_text? string  Pre-formatted TODO list text
---@field temporal_fire_count integer  Number of times temporal nudge has fired
---@field context_pressure_fired boolean  Whether context pressure has fired

---@class TodoSummary
---@field total integer
---@field completed integer
---@field incomplete integer

-- ============================================================================
-- NudgeResult (return type from composed pre_turn hooks)
-- ============================================================================

---@class NudgeResult
---@field text string  Nudge text to inject as system reminder
---@field temporal_fired boolean  Whether temporal nudge fired this turn
---@field context_pressure_fired boolean  Whether context pressure fired

-- ============================================================================
-- Agent config return type
-- ============================================================================

---@class AgentConfig
---@field name string  Agent name (required)
---@field description string  Short description (required)
---@field model? string  Model alias (nil = default)
---@field reasoning_effort? "low"|"medium"|"high"
---@field max_iterations? integer  Default 50
---@field tools string[]  Built-in tool names
---@field skills? string[]  Skill names to inject
---@field custom_tools? CustomToolDef[]
---@field build fun(ctx: AgentContext, args: table): BuildResult
---@field pre_turn? fun(state: PreTurnState): string|NudgeResult|nil
---@field on_end_turn? fun(state: PreTurnState): string|nil
---@field post_completion? fun(ctx: AgentContext)
---@field on_resume? fun(ctx: AgentContext)

---@class BuildResult
---@field system_prompt string
---@field messages BuildMessage[]

---@class BuildMessage
---@field role string
---@field content string
---@field tool_calls? table[]  Tool call descriptors (set by ctx:call_tools)
---@field tool_results? table[]  Tool result descriptors (set by ctx:call_tools)

---@class CustomToolDef
---@field name string
---@field description string
---@field parameters CustomToolParam[]
---@field terminal? boolean  If true, tool result ends the session
---@field handler fun(ctx: AgentContext, args: table): string

---@class CustomToolParam
---@field name string
---@field type string
---@field description string
---@field required? boolean

-- ============================================================================
-- ghost.nudges module (require("ghost.nudges"))
-- ============================================================================

---@class ghost.nudges
local nudges = {}

--- Compose multiple nudge functions into a single hook.
---@param ... fun(state: PreTurnState): string|NudgeResult|nil
---@return fun(state: PreTurnState): NudgeResult|nil
function nudges.compose(...) end

---@class IterationThreshold
---@field remaining integer  Trigger when remaining <= this value
---@field message string  Message template ({remaining}, {iteration} placeholders)

--- Warn at iteration milestones.
---@param thresholds IterationThreshold[]
---@return fun(state: PreTurnState): string|nil
function nudges.iteration_countdown(thresholds) end

---@class TemporalConfig
---@field after_seconds integer  Start firing after this many seconds
---@field messages string[]  Escalating messages ({minutes} placeholder)

--- Warn after elapsed time, with escalating messages.
---@param config TemporalConfig
---@return fun(state: PreTurnState): table|nil
function nudges.temporal(config) end

---@class ContextPressureConfig
---@field threshold_pct number  Fire when context usage exceeds this (0.0-1.0)
---@field message string  Warning message

--- Warn when context window usage exceeds threshold.
---@param config ContextPressureConfig
---@return fun(state: PreTurnState): table|nil
function nudges.context_pressure(config) end

---@class ProgressGateConfig
---@field no_todo string  Rejection message when no TODO exists
---@field incomplete string  Rejection message ({incomplete} placeholder)

--- Reject end-turn if TODO items are incomplete.
---@param config ProgressGateConfig
---@return fun(state: PreTurnState): string|nil
function nudges.progress_gate(config) end

--- Inject current TODO list text into the turn.
---@return fun(state: PreTurnState): string|nil
function nudges.todo_list() end

-- ============================================================================
-- ghost.template module (require("ghost.template"))
-- ============================================================================

---@class ghost.template
local template = {}

--- Render a template string, replacing {{key}} with values from vars.
---@param text string  Template with {{placeholder}} markers
---@param vars table<string, string|number>  Variable substitutions
---@return string
function template.render(text, vars) end
