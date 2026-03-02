local template = require("ghost.template")

return {
    name = "fork-reflection",
    description = "Knowledge extraction from completed agent sessions",

    max_iterations = 30,

    tools = {
        "run_shell_command",
        "read_file",
        "write_file",
        "file_edit",
        "knowledge_search",
        "note_write",
    },

    skills = { "note-writer" },

    build = function(ctx, args)
        local messages = ctx:list_messages(args.session_id)
        return {
            system_prompt = template.render(read_file("prompt.md"), {
                date = os.date("%Y-%m-%d"),
            }),
            messages = messages,
        }
    end,

    --- Post-completion: curate web cache references.
    post_completion = function(ctx)
        ctx:curate_web_cache()
    end,
}
