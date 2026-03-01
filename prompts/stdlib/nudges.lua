--- ghost.nudges — composable nudge functions for agent pre_turn / on_end_turn hooks.
---
--- Each nudge factory returns a function `(state) -> string|nil`. Use `compose()`
--- to combine multiple nudges into a single hook function.
---
--- Usage:
---   local nudges = require("ghost.nudges")
---   pre_turn = nudges.compose(
---       nudges.iteration_countdown({{remaining = 10, message = "..."}}),
---       nudges.temporal({after_seconds = 300, messages = {"hurry up"}}),
---       nudges.progress_gate({no_todo = "...", incomplete = "..."})
---   )

local M = {}

--- Interpolate placeholders like `{remaining}`, `{minutes}`, etc.
local function interpolate(text, vars)
    if not vars then return text end
    return (text:gsub("{(%w+)}", function(key)
        local val = vars[key]
        if val ~= nil then
            return tostring(val)
        end
        return "{" .. key .. "}"
    end))
end

--- Compose multiple nudge functions. Collects non-nil results and wraps them
--- in a single `<system-reminder>` block. Returns a table with `text` and
--- flag fields (`temporal_fired`, `context_pressure_fired`) for the Rust side.
--- Nudge functions may return a plain string or a table with `text` + flags.
---@param ... function  Nudge functions `(state) -> string|table|nil`
---@return function  `(state) -> table|nil`
function M.compose(...)
    local fns = {...}
    return function(state)
        local parts = {}
        local temporal_fired = false
        local context_pressure_fired = false
        for _, fn in ipairs(fns) do
            local result = fn(state)
            if result then
                if type(result) == "table" then
                    parts[#parts + 1] = result.text
                    if result.temporal_fired then temporal_fired = true end
                    if result.context_pressure_fired then context_pressure_fired = true end
                else
                    parts[#parts + 1] = result
                end
            end
        end
        if #parts == 0 then return nil end
        return {
            text = "<system-reminder>\n" .. table.concat(parts, "\n") .. "\n</system-reminder>",
            temporal_fired = temporal_fired,
            context_pressure_fired = context_pressure_fired,
        }
    end
end

--- Iteration countdown nudge. Fires the most urgent applicable rule.
---
--- Rules are sorted by `remaining` ascending; the one with the smallest
--- `remaining` value that is >= the current remaining fires.
---@param rules table[]  `{{remaining = 10, message = "..."}, ...}`
---@return function
function M.iteration_countdown(rules)
    -- Sort by remaining ascending at definition time
    table.sort(rules, function(a, b) return a.remaining < b.remaining end)

    return function(state)
        local remaining = state.remaining
        -- Find the most urgent (lowest threshold) rule that applies
        local best = nil
        for _, rule in ipairs(rules) do
            if remaining <= rule.remaining then
                best = best or rule
                -- Keep the one with the lowest remaining threshold
                if rule.remaining <= (best.remaining or math.huge) then
                    best = rule
                end
                break  -- already sorted, first match is the tightest
            end
        end
        if not best then return nil end
        return interpolate(best.message, { remaining = remaining })
    end
end

--- Wall-clock temporal nudge. Fires every iteration past `after_seconds`.
--- Supports escalating messages (list). `{minutes}` is interpolated.
---@param config {after_seconds: number, messages: string[]}
---@return function
function M.temporal(config)
    return function(state)
        if state.elapsed_seconds < config.after_seconds then
            return nil
        end
        local messages = config.messages
        local idx = math.min(state.temporal_fire_count + 1, #messages)
        local minutes = math.floor(state.elapsed_seconds / 60)
        return {
            text = interpolate(messages[idx], { minutes = minutes }),
            temporal_fired = true,
        }
    end
end

--- Tool count nudge. Shows count and optionally nudges if below minimum.
---@param config {tool: string, min: number|nil, message: string|nil}
---@return function
function M.tool_count(config)
    return function(state)
        local counts = state.tool_counts or {}
        local count = counts[config.tool] or 0
        if count == 0 then return nil end  -- don't nudge before first call

        local min = config.min
        local msg = config.message

        if msg and (not min or count < min) then
            return interpolate(msg, {
                tool = config.tool,
                count = count,
                min = min or "",
            })
        end
        return nil
    end
end

--- Recency nudge. Fires if a tool hasn't been used in the last N turns.
--- NOTE: Requires tool_counts in state; fine-grained per-turn tracking
--- is in the Rust layer — this is a simplified version.
---@param config {tool: string, window: number, message: string}
---@return function
function M.recency(config)
    return function(state)
        -- Simplified: fire if tool count is 0 and iteration > window
        local counts = state.tool_counts or {}
        local count = counts[config.tool] or 0
        if count > 0 or state.iteration < config.window then
            return nil
        end
        return config.message
    end
end

--- Context pressure nudge. Fires once when token usage exceeds threshold.
---@param config {threshold_pct: number, message: string}
---@return function
function M.context_pressure(config)
    return function(state)
        if state.context_pressure_fired then return nil end
        if state.context_window == 0 or state.last_input_tokens == 0 then
            return nil
        end
        local ratio = state.last_input_tokens / state.context_window
        if ratio < config.threshold_pct then return nil end
        return {
            text = config.message,
            context_pressure_fired = true,
        }
    end
end

--- TODO list injection nudge. Returns the pre-formatted TODO list from state.
--- Use in `pre_turn` to show the agent its current TODO progress.
---@return function
function M.todo_list()
    return function(state)
        return state.todo_text
    end
end

--- Progress gate for `on_end_turn`. Blocks EndTurn if TODO is missing or incomplete.
--- Returns non-nil (the rejection message) to block, nil to allow.
---@param config {no_todo: string, incomplete: string}
---@return function
function M.progress_gate(config)
    return function(state)
        -- If temporal nudge has fired, let the model end — don't contradict wrap-up signals
        if state.temporal_fire_count and state.temporal_fire_count > 0 then
            return nil
        end

        local todo = state.todo_summary
        if not todo then return nil end

        if todo.total == 0 then
            return config.no_todo
        end

        local incomplete = todo.incomplete or 0
        if incomplete > 0 then
            return interpolate(config.incomplete, { incomplete = incomplete })
        end

        return nil
    end
end

return M
