local template = require("ghost.template")

--- Bytes threshold for fitting transcript sections in a single user message.
--- Transcript imports are shorter than books, but we still keep a large
--- single-shot path for medium-sized videos and fall back to progressive
--- reading when the section set grows too large.
local MAX_SINGLE_SHOT_BYTES = 640000

return {
    name = "video-import",
    description = "Create structured notes from imported video transcript sections",

    max_iterations = 200,

    tools = {
        "file_read",
        "knowledge_search",
        "note_write",
        "shell",
    },

    skills = { "note-writer" },

    build = function(ctx, args)
        local topic = args.topic or error("video-import requires args.topic")
        local title = args.title or "Unknown"
        local channel = args.channel or "Unknown"

        local note_skill = load_skill("note-writer")

        -- List transcript section files with sizes, skipping metadata files.
        local ls_result = ctx:call_tool("shell", {
            command = "stat --format='%s %n' references/"
                .. topic
                .. "/*.md 2>/dev/null | grep -v '/_' | sort -t/ -k2 -V",
            timeout_ms = 10000,
        })

        local files = {}
        local total_bytes = 0
        for line in ls_result:gmatch("[^\n]+") do
            local size, path = line:match("^(%d+)%s+(.+)$")
            if size and path then
                size = tonumber(size)
                if size >= 200 then
                    table.insert(files, { size = size, path = path })
                    total_bytes = total_bytes + size
                end
            end
        end

        local header = "**Title**: "
            .. title
            .. "\n"
            .. "**Channel**: "
            .. channel
            .. "\n"
            .. "**Topic (reference path)**: "
            .. topic

        if total_bytes <= MAX_SINGLE_SHOT_BYTES then
            local transcript_result = ctx:call_tool("shell", {
                command = "cat references/" .. topic .. "/*.md",
                timeout_ms = 30000,
            })

            local system_prompt = template.render(read_file("prompt.md"), {
                note_skill = note_skill,
            })

            local user_message = "Create notes for the following video transcript.\n\n"
                .. header
                .. "\n\n## Full Transcript Sections\n\n"
                .. transcript_result
                .. "\n\n---\n\n"
                .. "The transcript sections are above. Create concise, well-linked notes from "
                .. "this video."

            return {
                system_prompt = system_prompt,
                messages = { { role = "user", content = user_message } },
            }
        else
            local manifest = ""
            for _, f in ipairs(files) do
                local kb = string.format("%.1f", f.size / 1024)
                manifest = manifest .. "- `" .. f.path .. "` (" .. kb .. " KB)\n"
            end

            local system_prompt = template.render(
                read_file("prompt-progressive.md"),
                { note_skill = note_skill }
            )

            local user_message = "Create notes for the following video transcript.\n\n"
                .. header
                .. "\n\n## Transcript Section Manifest\n\n"
                .. manifest
                .. "\nTotal: "
                .. string.format("%.0f", total_bytes / 1024)
                .. " KB across "
                .. #files
                .. " files\n\n---\n\n"
                .. "The transcript is too large to fit in context at once. Read the section "
                .. "files progressively using `file_read` and create notes as you go."

            return {
                system_prompt = system_prompt,
                messages = { { role = "user", content = user_message } },
            }
        end
    end,
}
