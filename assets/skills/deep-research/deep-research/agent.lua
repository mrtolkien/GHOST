local nudges = require("ghost.nudges")
local template = require("ghost.template")

return {
    name = "deep-research",
    description = "Iterative web research with full page reading and source evaluation",

    reasoning_effort = "high",
    max_iterations = 30,

    compaction = {
        keep_window = 8,
        instructions = "Preserve all URLs found during research, the current TODO list "
            .. "with completion status, search query history, and key findings. "
            .. "Drop verbose page content and raw search result listings.",
    },

    tools = {
        "knowledge_search",
        "web_search",
        "web_fetch",
        "file_read",
        "todo",
        "shell",
    },

    custom_tools = {
        report_findings = {
            description = "Submit your final research report. This ends your session. "
                .. "Include ALL information needed for downstream processing: the report "
                .. "itself, every source URL, secondary details, and negative findings.",
            parameters = {
                type = "object",
                properties = {
                    report = {
                        type = "string",
                        description = "The main research report in markdown. Lead with "
                            .. "the answer, use tables for comparisons, cite sources inline.",
                    },
                    sources = {
                        type = "array",
                        items = {
                            type = "object",
                            properties = {
                                url = { type = "string" },
                                title = { type = "string" },
                                contribution = {
                                    type = "string",
                                    description = "What this source contributed to the report",
                                },
                                quality = {
                                    type = "string",
                                    description = "Source quality assessment: hands-on testing, "
                                        .. "community-trusted, SEO listicle, affiliate, etc.",
                                },
                            },
                            required = { "url", "title", "contribution", "quality" },
                        },
                        description = "Sources that contributed useful information to the report. "
                            .. "Omit sources that were unhelpful, irrelevant, or low-quality.",
                    },
                    secondary_info = {
                        type = "string",
                        description = "Detailed specs, benchmarks, methodology notes, "
                            .. "and source quality analysis that support the report but "
                            .. "would clutter it. Include full spec tables, price breakdowns, "
                            .. "and any data points that informed your conclusions.",
                    },
                    negative_info = {
                        type = "string",
                        description = "Information that did NOT make it into the report "
                            .. "but was important for reaching the right answer: rejected "
                            .. "options and why, eliminated candidates, common misconceptions "
                            .. "corrected, edge cases ruled out, conflicting claims resolved.",
                    },
                },
                required = { "report", "sources", "secondary_info", "negative_info" },
            },
            handler = function(ctx, args)
                -- Encode the full structured data as JSON.
                local report_data = json.encode({
                    report = args.report,
                    sources = args.sources,
                    secondary_info = args.secondary_info,
                    negative_info = args.negative_info,
                })

                -- Persist for downstream consumers (daemon, tests).
                ctx:set("report_data", report_data)

                -- Spawn the reflection agent with the structured data.
                ctx:spawn_agent("deep-research-reflection", {
                    report_data = report_data,
                    session_id = ctx.session_id,
                })

                return report_data
            end,
            terminal = true,
        },
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
                    .. "remaining TODO items done or skipped and call `report_findings`.",
            },
            {
                remaining = 2,
                message = "FINAL WARNING: {remaining} iterations left. Your next response "
                    .. "MUST call `report_findings`. Do NOT call any other tools except `todo`.",
            },
        }),
        nudges.temporal({
            after_seconds = 300,
            messages = {
                "You've been working for {minutes} minutes. Start wrapping up: finish your "
                    .. "current tasks, mark remaining TODO items done or skipped, and call "
                    .. "`report_findings`.",
                "You've been working for {minutes} minutes. STOP starting new work. Call "
                    .. "`report_findings` NOW using what you have.",
                "FINAL WARNING ({minutes} min). Your next response MUST call "
                    .. "`report_findings`. Do NOT call any other tools.",
            },
        }),
        nudges.context_pressure({
            threshold_pct = 0.80,
            message = "Your context window is over 80% full. Wrap up your remaining TODO "
                .. "items and call `report_findings` using what you have. Do not start "
                .. "new searches.",
        })
    ),

    on_end_turn = nudges.progress_gate({
        no_todo = "REJECTED — create a TODO plan before proceeding.",
        incomplete = "REJECTED — you have {incomplete} incomplete TODO item(s). Complete or "
            .. "mark them done/skipped before calling `report_findings`.",
    }),

    -- Reflection spawning happens inside the report_findings handler.
    -- No post_completion needed — the handler spawns deep-research-reflection
    -- with structured data before the terminal tool ends the session.
}
