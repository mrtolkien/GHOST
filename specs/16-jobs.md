# 16 — Job System: Scheduling and Execution

## Overview

Jobs are markdown files in `$WORKSPACE/jobs/` with TOML frontmatter. They define
cron-scheduled tasks that the GHOST executes autonomously.

For the PoC, the job system handles **cron jobs only**. Heartbeat and reflection are
dedicated subsystems with their own code paths (see
[17-default-jobs.md](17-default-jobs.md)).

> **Future**: The end goal is a unified job system where ALL autonomous behaviors —
> including heartbeat and reflection — are expressed as jobs with event-based triggers
> (session_idle, job_completed, daemon_start, etc.). This requires the Lua jobs system
> (see `backlog/lua-jobs.md`) which provides the flexibility needed for complex jobs
> like reflection. The cron job system built here is the foundation that the unified
> system will extend.

## Job File Format

```markdown
+++
name = "weekly-research"
enabled = true
schedule = "0 9 * * MON"
model = "primary"
tools = "chat"
+++

# Weekly Research

Review the OPERATOR's current projects and search for relevant news, updates, and
resources. Create or update notes for anything significant.

Focus on:

- Active project dependencies and their changelogs
- Industry news related to current work
- New tools or libraries that might be useful
```

### Frontmatter Fields

| Field               | Type   | Required | Default     | Description                       |
| ------------------- | ------ | -------- | ----------- | --------------------------------- |
| `name`              | string | yes      |             | Unique job identifier             |
| `enabled`           | bool   | no       | `true`      | Whether the job is active         |
| `schedule`          | string | yes      |             | Cron expression (5-field, UTC)    |
| `model`             | string | no       | `"default"` | Model alias from config           |
| `tools`             | string | no       | `"chat"`    | Tool set: "chat", "none"          |
| `carry_last_output` | bool   | no       | `false`     | Load/save `.state/<name>.last.md` |

## Cron Schedules

Standard 5-field cron expressions (UTC):

```toml
schedule = "0 9 * * MON" # Every Monday at 9am UTC
schedule = "*/30 * * * *" # Every 30 minutes
schedule = "0 0 1 * *" # First of every month
```

## Module Structure

The `jobs/` module houses the cron scheduler and the heartbeat/reflection subsystems:

```
src/jobs/
├── mod.rs            # re-exports
├── scheduler.rs      # Cron job loading, tick loop, file watching
├── definition.rs     # JobDefinition, frontmatter parsing
├── heartbeat.rs      # HeartbeatManager (see 17-default-jobs.md)
└── reflection.rs     # ReflectionManager (see 17-default-jobs.md)
```

## Scheduler

The scheduler runs inside the daemon process and manages all cron jobs:

```rust
pub struct Scheduler {
    jobs: Vec<LoadedJob>,
    db: Surreal<Db>,
    session_chat: Arc<SessionChat>,
}

pub struct LoadedJob {
    pub definition: JobDefinition,
    pub next_run: Option<DateTime<Utc>>,
    pub last_run: Option<DateTime<Utc>>,
}
```

### Scheduler Loop

1. On daemon start, load all job files from `$WORKSPACE/jobs/`
2. Watch `$WORKSPACE/jobs/` for file changes (add, modify, delete)
3. Every tick (default: 10 seconds): a. Check cron jobs — is it time to run? b. Execute
   eligible jobs via `SessionChat::chat_job()`

### File Watching

Use the `notify` crate to watch `$WORKSPACE/jobs/` for changes. When a job file is
modified:

- Re-parse the job definition
- Update the loaded job in the scheduler
- Log the change

When a job file is deleted, remove it from the scheduler.

## Job Execution

Cron jobs run in a **clean context** (no session history). They are standalone prompts:

1. The job's markdown body (everything after the frontmatter) becomes the prompt
2. If `carry_last_output`, the previous output is prepended as context
3. The provider runs with the specified tool set
4. The full transcript is saved to the `job_log` table
5. If `carry_last_output`, the response is saved to `.state/<name>.last.md`

Cron jobs do NOT run inside an existing session — they create a one-off chat with no
message history beyond the job prompt itself.

## Job State

For jobs with `carry_last_output = true`:

```
$WORKSPACE/.state/
├── weekly-research.last.md
└── ...
```

The previous output is injected into the job prompt on the next run, enabling
continuity.

## CLI Commands

- `ghost job list` — List all jobs with status, next run, and last run
- `ghost job validate <path>` — Validate a job file's frontmatter and cron syntax
- `ghost job run <name>` — Run a job manually, outside the scheduler
- `ghost job logs [name]` — Show recent job logs (optionally filtered by name)

## Observability

```rust
#[tracing::instrument(skip_all, fields(
    job_name = %job.name,
))]
async fn execute_job(&self, job: &LoadedJob) -> Result<()> {
    logfire::info!("job started", job_name = %job.name);
    // ...
    logfire::info!("job completed",
        job_name = %job.name,
        status = %status,
        duration_ms = elapsed.as_millis(),
    );
}
```

## Acceptance Criteria

- Job files in `$WORKSPACE/jobs/` are loaded on daemon start
- Cron triggers fire at the correct times
- `carry_last_output` loads/saves state between runs
- File watcher picks up job changes without restart
- `ghost job validate` checks frontmatter syntax
- `ghost job run` executes a job manually
- All job operations produce tracing spans
- Job transcripts are stored in `job_log` table
- `just ci` passes

## Prior Art

Old code in `../t-koma`:

- `t-koma-gateway/src/scheduler.rs` — Scheduler loop with tick-based checking. Reusable
  pattern.
- `t-koma-gateway/src/cron.rs` — Cron job execution and file watching. Directly
  reusable.
- `t-koma-core/src/cron.rs` — TOML frontmatter parsing for job files. Directly reusable.
