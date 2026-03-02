local template = require("ghost.template")

return {
    name = "deep-research-reflection",
    description = "Knowledge extraction from structured research reports",

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

        local user_message = template.render(read_file("prompt.md"), {
            date = os.date("%Y-%m-%d"),
            report = report_data.report or "(no report)",
            sources = table.concat(source_lines, "\n\n"),
            secondary_info = report_data.secondary_info or "(none)",
            negative_info = report_data.negative_info or "(none)",
        })

        return {
            system_prompt = "You are a knowledge extraction agent. Today is "
                .. os.date("%Y-%m-%d")
                .. ". Your job is to turn structured "
                .. "research reports into well-organized knowledge notes. "
                .. "Read the note-writer skill for formatting instructions.",
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
