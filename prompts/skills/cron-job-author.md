---
name: cron-job-author
description:
  Create and improve file-based cron jobs. Use when the OPERATOR asks for scheduled
  automation via jobs in $WORKSPACE/jobs/.
triggers:
  - schedule a job
  - create a cron job
  - automate this
---

# Cron Job Author

Use this skill when the OPERATOR asks for scheduled automation.

## Goal

Design cron jobs that are useful immediately and improve over time.

Every cron prompt should:

- clearly restate the OPERATOR's intent and success criteria,
- use focused pre-model tool calls for deterministic input data,
- explicitly allow self-improvement when there is a clear, unambiguous improvement path.

## Where Jobs Live

Jobs are markdown files in:

- `$WORKSPACE/jobs/*.md`

State from the previous run may be carried by the runtime automatically.

## File Format

Use TOML frontmatter delimited by `+++`:

```markdown
+++
name = "Daily recap"
schedule = "0 8 * * *"
enabled = true
carry_last_output = true
pre_tools = [
  { name = "web_fetch", input = { url = "https://example.com/feed.xml" } }
]
+++

Explain the OPERATOR intent clearly. State what "good output" means. If you detect a
clear and low-risk improvement, update this job file's prompt and/or pre_tools for next
runs.
```

`carry_last_output` controls whether the last successful output of this same job is
injected into the next run as context (`true` = keep continuity, `false` = stateless).

## Scheduling Rules

- Use standard 5-field cron syntax.
- Schedule is interpreted in UTC.
- Missed runs are skipped when the system is down.

## Prompt-Writing Guidance

Include these elements in the job prompt body:

1. The OPERATOR intent in concrete terms.
2. What inputs are expected from pre-model tools.
3. Required output format and brevity level.
4. A constrained self-improvement instruction: "If you find a clear, unambiguous
   improvement, update this job's prompt and/or pre_tools to improve future runs. Do not
   make speculative or broad changes."

## Safety and Quality Guardrails

- Prefer small edits over rewrites when self-improving.
- Keep scope tight to the job objective.
- Avoid adding extra pre-tools unless they clearly improve signal quality.
- Keep the prompt deterministic and testable.

## Validate Before Finishing

Use the CLI validator before finalizing job file edits:

- `ghost job validate $WORKSPACE/jobs/my-job.md`
