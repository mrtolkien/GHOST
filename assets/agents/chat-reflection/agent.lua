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
        local session_id = ctx.trigger_session_id
        assert(
            session_id and session_id ~= "",
            "chat-reflection requires a trigger session"
        )

        local since = ctx:get("last_reflected_at")
        local transcript = ctx:filter_transcript(session_id, since)
        local diary = ctx:load_diary_today() or "No diary entry yet."

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

        -- Build user message from template.
        local user_message = template.render(read_file("user-message.md"), {
            diary = diary,
            transcript = transcript,
        })

        return {
            system_prompt = system_prompt,
            messages = {
                { role = "user", content = user_message },
            },
        }
    end,

    --- Post-completion: record timestamp and curate web cache.
    post_completion = function(ctx)
        ctx:set("last_reflected_at", os.date("!%Y-%m-%dT%H:%M:%SZ"))
        if ctx.trigger_session_id then
            ctx:curate_web_cache(ctx.trigger_session_id)
        end
    end,
}
