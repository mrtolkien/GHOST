local template = require("ghost.template")

return {
    name = "book-import",
    description = "Create structured notes from imported book chapters",

    max_iterations = 40,
    reasoning_effort = "high",

    tools = {
        "file_read",
        "knowledge_search",
        "note_write",
        "shell",
    },

    skills = { "note-writer" },

    build = function(ctx, args)
        local topic = args.topic or error("book-import requires args.topic")
        local title = args.title or "Unknown"
        local authors = args.authors or "Unknown"

        local note_skill = load_skill("note-writer")
        local system_prompt = template.render(read_file("prompt.md"), {
            note_skill = note_skill,
        })

        local user_message = "Create notes for the following book.\n\n"
            .. "**Title**: " .. title .. "\n"
            .. "**Author(s)**: " .. authors .. "\n"
            .. "**Topic (reference path)**: " .. topic .. "\n\n"
            .. "Chapters are at `references/" .. topic .. "/`. "
            .. "Start by listing the files, then read them all."

        return {
            system_prompt = system_prompt,
            messages = { { role = "user", content = user_message } },
        }
    end,
}
