local template = require("ghost.template")

return {
    name = "fork-reflection",
    description = "Knowledge extraction from completed agent sessions",

    max_iterations = 30,
    trigger = "after_agent",
    continue_trigger_session = true,

    tools = {
        "run_shell_command", "read_file", "write_file",
        "file_edit", "knowledge_search", "note_write",
    },

    skills = { "note-writer" },

    system_prompt = template.render(read_file("prompt.md"), {
        date = os.date("%Y-%m-%d"),
    }),

    --- Skip if the completed agent is itself a reflection agent.
    should_trigger = function(ctx)
        local trigger_agent = ctx.trigger_agent_name or ""
        if trigger_agent == "reflection" or trigger_agent == "fork-reflection" then
            return false
        end
        return true
    end,

    --- Post-completion: curate web cache references.
    post_completion = function(ctx)
        ctx:curate_web_cache()
    end,
}
