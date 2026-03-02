local template = require("ghost.template")

return {
    name = "chat-reflection",
    description = "Reflection on operator chat sessions",

    max_iterations = 30,

    tools = {
        "run_shell_command",
        "read_file",
        "write_file",
        "file_edit",
        "knowledge_search",
        "note_write",
    },

    skills = { "note-writer", "knowledge-navigator" },

    build = function(ctx, args)
        -- Build system prompt: agent prompt + inlined skills.
        local base_prompt = template.render(read_file("prompt.md"), {
            date = os.date("%Y-%m-%d"),
        })
        local note_skill = load_skill("note-writer")
        local nav_skill = load_skill("knowledge-navigator")
        local system_prompt = base_prompt
            .. "\n\n---\n\n"
            .. note_skill
            .. "\n\n---\n\n"
            .. nav_skill

        return {
            system_prompt = system_prompt,
            messages = {
                { role = "user", content = args.prompt or "Begin reflection." },
            },
        }
    end,
}
