local template = require("ghost.template")

return {
    name = "coding-quality-reviewer",
    description = "Review code quality after spec compliance is confirmed",

    max_iterations = 20,

    tools = {
        "file_read",
        "shell",
    },

    custom_tools = {
        submit_review = {
            description = "Submit your code quality review. This ends your session.",
            parameters = {
                type = "object",
                properties = {
                    approved = {
                        type = "boolean",
                        description = "Whether the code quality is acceptable.",
                    },
                    issues = {
                        type = "string",
                        description = "Quality issues found. Empty string if approved.",
                    },
                },
                required = { "approved", "issues" },
            },
            handler = function(ctx, args)
                local result = json.encode({
                    approved = args.approved,
                    issues = args.issues,
                })
                ctx:set("review_result", result)
                return result
            end,
            terminal = true,
        },
    },

    build = function(ctx, args)
        return {
            system_prompt = template.render(read_file("prompt.md"), {
                scope = args.scope or args.prompt or "Review all recent changes.",
            }),
            messages = {
                { role = "user", content = "Review the code quality." },
            },
        }
    end,
}
