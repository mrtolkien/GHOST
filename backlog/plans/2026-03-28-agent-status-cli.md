# Agent Status CLI Commands

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or superpowers:executing-plans
> to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add CLI commands to view running agents, recent run history, and detailed
results for individual runs.

**Architecture:** Two new subcommands under `ghost agent` — `status` (table of running +
recent runs) and `show` (detail view of a single run). DB-only queries, no daemon
communication. Extends the existing `AgentCommand` enum in `src/cli/agent.rs`.

**Tech Stack:** clap (CLI), sqlx (DB queries), existing `db::agent_runs` +
`db::sessions` modules.

**Addresses:** quickfix item "no way to see direct agent message" from
`backlog/tasks/00-quickfixes.md` line 8.

---

## Commands

### `ghost agent status`

Table of running agents + recent completed/failed runs.

```
$ ghost agent status

ID        AGENT            STATUS    STARTED              DURATION
01JQ8A2B  deep-research    running   2026-03-28 14:02:31  12m 34s
01JQ7F3C  chat-reflection  ok        2026-03-28 13:30:00  2m 11s
01JQ3D1A  daily-summary    ok        2026-03-28 03:00:00  45s
01JQ1E4F  deep-research    failed    2026-03-27 22:15:00  8m 03s
```

- Running agents first (sorted by start time desc), then recent finished runs
- Default limit: 20 rows
- Flags:
  - `--agent <name>` — filter to a specific agent
  - `--limit <n>` — override default 20

### `ghost agent show <run_id>`

Detail view of a single agent run.

```
$ ghost agent show 01JQ8A2B

Agent: deep-research
Status: ok
Started: 2026-03-28 14:02:31
Finished: 2026-03-28 14:14:42
Duration: 12m 11s

<transcript/findings text>
```

- Default: metadata header + transcript/findings only
- **Failed runs:** automatically show the full error — last assistant message from the
  session (which typically contains the error/failure reason) printed after the header,
  no `--full` needed
- `--full`: dumps the full session message history (every user/assistant message, tool
  calls summarized as one-line descriptions)
- `--json`: structured JSON output of the run record + transcript. Combines with
  `--full` to include full message history in JSON.
- Accepts full ULID or unique prefix (minimum 4 chars)

### Existing commands (unchanged)

- `ghost agent list` — discovered agents from filesystem
- `ghost agent validate` — validate agent Lua configs

---

## Data Source

All queries hit SQLite directly. No daemon IPC.

### Queries needed

**For `status`:** `list_runs(agent_name: Option<&str>, limit: i64)` — already exists in
`db::agent_runs`. Needs ordering adjustment: `running` status first, then by
`started_at DESC`.

**For `show`:** `get_run(run_id: &str)` — already exists. For `--full`, also query
session messages via `agent_session_id` using existing `db::sessions` functions.

**Prefix matching for run IDs:** `get_run` needs a variant that does
`WHERE id LIKE '{prefix}%'` and returns an error if zero or multiple matches.

---

## File Map

**Modified files:**

- `src/cli/agent.rs` — add `Status` and `Show` variants to `AgentCommand`, implement
  `execute()` handlers
- `src/db/agent_runs.rs` — add `list_runs_for_status()` (running-first ordering), add
  `get_run_by_prefix()` (prefix matching)

**No new files needed.** All logic fits in the existing agent CLI and DB modules.

---

## Run ID Display

`agent_run.id` is a ULID (26 chars). Table shows first 8 characters. `show` accepts
either:

- Full ULID
- Unique prefix (minimum 4 chars, error if ambiguous)

---

## JSON Output Schema (`--json`)

```json
{
  "id": "01JQ8A2B...",
  "agent_name": "deep-research",
  "status": "ok",
  "started_at": "2026-03-28T14:02:31Z",
  "finished_at": "2026-03-28T14:14:42Z",
  "transcript": "...",
  "messages": []
}
```

- `messages` field only present when `--full` is also passed
- Each message:
  `{ "role": "user"|"assistant"|"system", "content": "...", "tool_calls": [...] }`

---

## Implementation Steps

- [ ] **Step 1:** Add `list_runs_for_status()` to `src/db/agent_runs.rs` — query with
      running-first ordering, optional agent name filter, limit param
- [ ] **Step 2:** Add `get_run_by_prefix()` to `src/db/agent_runs.rs` — prefix match
      with ambiguity error
- [ ] **Step 3:** Add `Status` and `Show` variants to `AgentCommand` in
      `src/cli/agent.rs` with clap attributes for flags
- [ ] **Step 4:** Implement `status` handler — DB query, format as table, print to
      stdout
- [ ] **Step 5:** Implement `show` handler — prefix lookup, print header + transcript,
      handle `--full` (session messages) and `--json` flags
- [ ] **Step 6:** Run `just ci`, fix any issues
