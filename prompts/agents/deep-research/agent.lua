local nudges = require("ghost.nudges")
local template = require("ghost.template")

return {
    name = "deep-research",
    description = "Iterative web research with full page reading and source evaluation",

    reasoning_effort = "high",
    max_iterations = 30,

    tools = {
        "knowledge_search", "web_search", "web_fetch",
        "read_file", "todo", "note_write", "run_shell_command",
    },

    build = function(ctx, args)
        return {
            system_prompt = template.render(read_file("prompt.md"), {
                date = os.date("%Y-%m-%d"),
            }),
            messages = {
                { role = "user", content = args.prompt or "Begin research." },
            },
        }
    end,

    pre_turn = nudges.compose(
        nudges.todo_list(),
        nudges.iteration_countdown({
            {
                remaining = 10,
                message = "You have {remaining} iterations left. Prioritize: complete your "
                    .. "highest-value remaining TODO items and skip low-priority ones.",
            },
            {
                remaining = 5,
                message = "Only {remaining} iterations left. Stop starting new work. Mark "
                    .. "remaining TODO items done or skipped and write your final message.",
            },
            {
                remaining = 2,
                message = "FINAL WARNING: {remaining} iterations left. Your next response "
                    .. "MUST be your final message text. Do NOT call any tools except `todo`.",
            },
        }),
        nudges.temporal({
            after_seconds = 300,
            messages = {
                "You've been working for {minutes} minutes. Start wrapping up: finish your "
                    .. "current tasks, mark remaining TODO items done or skipped, and write "
                    .. "your final message.",
                "You've been working for {minutes} minutes. STOP starting new work. Write "
                    .. "your final message NOW using what you have.",
                "FINAL WARNING ({minutes} min). Your next response MUST be your final "
                    .. "message text. Do NOT call any tools. Write your message immediately.",
            },
        }),
        nudges.context_pressure({
            threshold_pct = 0.80,
            message = "Your context window is over 80% full. Wrap up your remaining TODO "
                .. "items and write your final report using what you have. Do not start "
                .. "new searches.",
        })
    ),

    on_end_turn = nudges.progress_gate({
        no_todo = "REJECTED — create a TODO plan before proceeding.",
        incomplete = "REJECTED — you have {incomplete} incomplete TODO item(s). Complete or "
            .. "mark them done/skipped before writing your final message.",
    }),

    post_completion = function(ctx)
        ctx:spawn_agent("fork-reflection", { session_id = ctx.session_id })
    end,
}
