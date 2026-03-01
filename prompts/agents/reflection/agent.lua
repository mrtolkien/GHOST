local template = require("ghost.template")

return {
    name = "chat-reflection",
    description = "Reflection on operator chat sessions",

    max_iterations = 30,
    trigger = "after_idle",
    idle_minutes = 30,

    tools = {
        "run_shell_command", "read_file", "write_file",
        "file_edit", "knowledge_search", "note_write",
    },

    skills = { "knowledge-navigator", "note-writer" },

    system_prompt = template.render(read_file("prompt.md"), {
        date = os.date("%Y-%m-%d"),
    }),
}
