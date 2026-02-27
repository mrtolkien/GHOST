---
name: e2e-testing
description: >-
  Step-based e2e test design and implementation for Ghost. Use this skill when creating,
  rewriting, or reviewing end-to-end tests that chain fixtures across steps, use
  LiveTestEnv snapshots, run sequentially, or compare behavior across model aliases.
  Covers: tests/e2e harness layout, hard-fail predecessor fixtures, transcript/metrics
  artifacts, and interactive scripts/e2e tooling.
---

# E2E Testing (Step-Based)

## When To Read This Skill

Read this skill before making changes if the task mentions any of:

- e2e tests, live integration tests, or scenario tests
- splitting one flow into multiple steps
- fixture chaining or snapshot restore between tests
- multi-model e2e runs (`GHOST_E2E_MODEL`)
- transcript/log rendering or fixture diffs

If the task is only unit tests or basic integration tests that do not use the e2e
harness, use the `testing` skill only.

## Current E2E System

Entrypoint and modules:

- `tests/e2e_steps.rs`
- `tests/e2e/harness.rs`
- `tests/e2e/scenarios/<scenario>/step_XX_*.rs`

Scenario currently implemented:

- `printer_3d`
  - `step_01_spawn_agent`
  - `step_02_run_agent_completion`
  - `step_03_reflect_agent`
  - `step_04_finalize_chat_and_reflect`

## Non-Negotiable Rules

- One test per action boundary (one step = one responsibility)
- Steps after step 01 must load predecessor snapshot
- Missing predecessor snapshot is a hard failure
- Sequential execution only (`--test-threads=1`)
- Full workspace snapshot is persisted as `workspace.tar.zst`

## Fixture Contract

Fixture path:

```text
tests/fixtures/e2e/<scenario>/<model_alias>/step_XX_<name>/
```

Required artifacts per step:

- `workspace.tar.zst`
- `state.json`
- `transcript.json`
- `transcript.md`
- `metrics.json`

`state.json` must include enough linkage for next step:

- `scenario`, `model_alias`, `step`, `parent_step`
- `chat_session_id`
- optional `agent_id` and `agent_session_id`
- `assertion_markers` with step outputs needed later

## Creating A New E2E Scenario

1. Add `tests/e2e/scenarios/<scenario>/mod.rs` and step files.
2. Keep each step readable: setup -> action -> assertions -> save snapshot.
3. Use `harness::load_previous_step_or_fail(...)` for steps 02+.
4. Build new state via `harness::fresh_step_state(...)`.
5. Persist with `harness::save_step_snapshot(...)`.
6. Ensure assertions are structural and robust across model variance.

## Running And Refreshing

Manual refresh (interactive or explicit):

```sh
uv run scripts/e2e
uv run scripts/e2e refresh --models primary,openai
```

Run one step directly:

```sh
GHOST_E2E_MODEL=primary cargo test --features e2e-tests --test e2e_steps printer_3d_step_01_spawn_agent -- --nocapture --test-threads=1
```

Inspect logs and diffs:

```sh
uv run scripts/e2e render-log
uv run scripts/e2e diff
uv run scripts/e2e analyze-request
```

## Review Checklist

- Does each step do exactly one thing?
- Are predecessor fixtures enforced with hard-fail?
- Is state handoff explicit and minimal?
- Are transcript markdown and metrics produced?
- Can the flow run sequentially for multiple model aliases?
