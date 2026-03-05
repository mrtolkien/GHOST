local template = require("ghost.template")

return {
    name = "coding-spec-reviewer",
    description = "Review implementation against spec for compliance",

    max_iterations = 20,

    tools = {
        "read_file",
        "run_shell_command",
    },

    custom_tools = {
        submit_review = {
            description = "Submit your spec compliance review. This ends your session.",
            parameters = {
                type = "object",
                properties = {
                    compliant = {
                        type = "boolean",
                        description = "Whether the implementation matches the spec.",
                    },
                    issues = {
                        type = "string",
                        description = "Missing requirements, extra additions, or "
                            .. "deviations from spec. Empty string if compliant.",
                    },
                },
                required = { "compliant", "issues" },
            },
            handler = function(ctx, args)
                local result = json.encode({
                    compliant = args.compliant,
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
                task_text = args.task_text or args.prompt or "No spec provided.",
            }),
            messages = {
                {
                    role = "user",
                    content = "Review the implementation against the spec.",
                },
            },
        }
    end,
}
