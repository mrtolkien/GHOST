local template = require("ghost.template")

return {
    name = "coding-implementer",
    description = "Implement a single task following TDD",

    max_iterations = 50,

    compaction = {
        keep_window = 10,
        instructions = "Preserve: current task text, files modified, test results, "
            .. "decisions made. Drop: verbose file contents, raw shell output from "
            .. "successful commands.",
    },

    tools = {
        "read_file",
        "write_file",
        "file_edit",
        "run_shell_command",
        "todo",
    },

    build = function(ctx, args)
        return {
            system_prompt = template.render(read_file("prompt.md"), {
                task_text = args.task_text or args.prompt or "No task specified.",
                context = args.context or "",
            }),
            messages = {
                { role = "user", content = args.task_text or args.prompt or "Begin." },
            },
        }
    end,
}
