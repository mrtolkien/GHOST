local template = require("ghost.template")

return {
    name = "deep-research-reflection",
    description = "Knowledge extraction from structured research reports",

    max_iterations = 30,

    tools = {
        "shell",
        "file_read",
        "file_write",
        "file_edit",
        "knowledge_search",
        "note_write",
    },

    skills = { "note-writer", "knowledge-navigator" },

    build = function(ctx, args)
        -- Parse the structured report data from the research agent.
        local report_data = json.decode(args.report_data)

        -- Format sources as a readable list.
        local source_lines = {}
        if report_data.sources then
            for i, src in ipairs(report_data.sources) do
                table.insert(
                    source_lines,
                    string.format(
                        "%d. [%s](%s)\n   Contribution: %s\n   Quality: %s",
                        i,
                        src.title or "Untitled",
                        src.url or "",
                        src.contribution or "—",
                        src.quality or "unknown"
                    )
                )
            end
        end

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

        -- Build user message: pure data template.
        local user_message = template.render(read_file("user-message.md"), {
            report = report_data.report or "(no report)",
            sources = table.concat(source_lines, "\n\n"),
            secondary_info = report_data.secondary_info or "(none)",
            negative_info = report_data.negative_info or "(none)",
        })

        return {
            system_prompt = system_prompt,
            messages = {
                { role = "user", content = user_message },
            },
        }
    end,

    --- Post-completion: curate web cache references.
    post_completion = function(ctx)
        ctx:curate_web_cache()
    end,
}
