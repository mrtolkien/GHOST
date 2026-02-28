---
title: Jobs
description: Cron-scheduled tasks, heartbeat, and reflection subsystems.
---

Jobs are cron-scheduled tasks defined as markdown files with YAML frontmatter.

## Job Format

Jobs live in `$WORKSPACE/jobs/`:

```markdown title="jobs/daily-news.md"
---
name: Daily News Summary
schedule: "0 8 * * *"
enabled: true
model: primary
tools:
  - web_search
  - web_fetch
  - read_file
carry_last_output: true
pre_tools:
  - name: web_fetch
    input:
      url: https://news.ycombinator.com
---

Summarize the top stories from Hacker News. Focus on AI and systems programming. Write a
brief digest with links.
```

### Fields

| Field               | Required | Description                                          |
| ------------------- | -------- | ---------------------------------------------------- |
| `name`              | yes      | Display name                                         |
| `schedule`          | yes      | Cron expression (e.g., `0 8 * * *` for daily at 8am) |
| `enabled`           | no       | Default: true                                        |
| `model`             | no       | Model alias (defaults to primary)                    |
| `tools`             | no       | Tool whitelist                                       |
| `carry_last_output` | no       | Pass previous run's output as context                |
| `pre_tools`         | no       | Tools to run before the main prompt                  |

## Built-in Subsystems

### Heartbeat

Proactive check-ins when you've been idle. These are dedicated subsystems (not regular
jobs) with their own scheduling logic.

:::tip
Configure heartbeat timing in `config.toml` to match your workflow — shorter
idle times mean more proactive check-ins.
:::

```toml title="~/.config/ghost/config.toml"
[timing]
heartbeat_idle_minutes = 4 # Minutes of silence before check-in
heartbeat_check_seconds = 60 # How often to check
heartbeat_continue_minutes = 30 # Max heartbeat duration
```

### Reflection

Automatic knowledge extraction after conversations and agent research.
See the dedicated [Reflection](/features/reflection/) page for details.

```toml title="~/.config/ghost/config.toml"
[timing]
reflection_idle_minutes = 4 # Minutes idle before reflection triggers
```

## CLI Commands

```bash
# List all jobs with next-run times
ghost job list

# Validate a job file
ghost job validate jobs/daily-news.md

# Run a job immediately
ghost job run "Daily News Summary"

# View job logs
ghost job logs
ghost job logs "Daily News Summary"
```
