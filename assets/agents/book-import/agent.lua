local template = require("ghost.template")

--- Bytes threshold for fitting book content in a single user message.
--- ~80% of 200K-token context window at ~4 bytes/token = 640KB.
--- Leaves room for system prompt, tools, and output tokens.
local MAX_SINGLE_SHOT_BYTES = 640000

return {
    name = "book-import",
    description = "Create structured notes from imported book chapters",

    max_iterations = 200,

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

        -- List chapter files with sizes (skip _ prefixed metadata files)
        local ls_result = ctx:call_tool("shell", {
            command = "stat --format='%s %n' references/"
                .. topic
                .. "/*.md 2>/dev/null | grep -v '/_' | sort -t/ -k2 -V",
            timeout_ms = 10000,
        })

        -- Parse file list and compute total size
        local files = {}
        local total_bytes = 0
        for line in ls_result:gmatch("[^\n]+") do
            local size, path = line:match("^(%d+)%s+(.+)$")
            if size and path then
                size = tonumber(size)
                if size >= 200 then -- skip trivial files (covers, part dividers)
                    table.insert(files, { size = size, path = path })
                    total_bytes = total_bytes + size
                end
            end
        end

        local header = "**Title**: "
            .. title
            .. "\n"
            .. "**Author(s)**: "
            .. authors
            .. "\n"
            .. "**Topic (reference path)**: "
            .. topic

        if total_bytes <= MAX_SINGLE_SHOT_BYTES then
            -- Tier 1: fits in context — dump everything in one shot
            local book_result = ctx:call_tool("shell", {
                command = "cat references/" .. topic .. "/*.md",
                timeout_ms = 30000,
            })

            local system_prompt = template.render(read_file("prompt.md"), {
                note_skill = note_skill,
            })

            local user_message = "Create notes for the following book.\n\n"
                .. header
                .. "\n\n## Full Book Text\n\n"
                .. book_result
                .. "\n\n---\n\n"
                .. "The full text is above. Now create the notes."

            return {
                system_prompt = system_prompt,
                messages = { { role = "user", content = user_message } },
            }
        else
            -- Tier 2: too large — progressive chapter-by-chapter processing
            local manifest = ""
            for _, f in ipairs(files) do
                local kb = string.format("%.1f", f.size / 1024)
                manifest = manifest .. "- `" .. f.path .. "` (" .. kb .. " KB)\n"
            end

            local system_prompt = template.render(
                read_file("prompt-progressive.md"),
                { note_skill = note_skill }
            )

            local user_message = "Create notes for the following book.\n\n"
                .. header
                .. "\n\n## Chapter Manifest\n\n"
                .. manifest
                .. "\nTotal: "
                .. string.format("%.0f", total_bytes / 1024)
                .. " KB across "
                .. #files
                .. " files\n\n---\n\n"
                .. "The book is too large to fit in context at once. "
                .. "Read chapters progressively using `file_read` and create notes as you go."

            return {
                system_prompt = system_prompt,
                messages = { { role = "user", content = user_message } },
            }
        end
    end,
}
