local template = require("ghost.template")

return {
    name = "coding-reviewer",
    description = "Full code review for production readiness",

    max_iterations = 20,

    tools = {
        "file_read",
        "shell",
    },

    custom_tools = {
        submit_review = {
            description = "Submit your code review. This ends your session.",
            parameters = {
                type = "object",
                properties = {
                    ready_to_merge = {
                        type = "string",
                        enum = { "yes", "no", "with_fixes" },
                        description = "Whether the code is ready to merge.",
                    },
                    strengths = {
                        type = "string",
                        description = "What's well done. Be specific with file:line references.",
                    },
                    issues = {
                        type = "string",
                        description = "Issues found, categorized by severity "
                            .. "(Critical/Important/Minor). Empty string if none.",
                    },
                    assessment = {
                        type = "string",
                        description = "Technical assessment in 1-2 sentences.",
                    },
                },
                required = { "ready_to_merge", "strengths", "issues", "assessment" },
            },
            handler = function(ctx, args)
                local result = json.encode({
                    ready_to_merge = args.ready_to_merge,
                    strengths = args.strengths,
                    issues = args.issues,
                    assessment = args.assessment,
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
                what_was_implemented = args.what_was_implemented
                    or args.prompt
                    or "Review recent changes.",
                description = args.description or "",
                plan_reference = args.plan_reference or "",
                base_sha = args.base_sha or "HEAD~1",
                head_sha = args.head_sha or "HEAD",
            }),
            messages = {
                { role = "user", content = "Review the code changes." },
            },
        }
    end,
}
