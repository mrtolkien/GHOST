# Better E2E Test Flow

This spec defines the step-based e2e harness used for live scenario tests.

## Goals

- One test per action boundary
- Persist full workspace between steps
- Make logs easy to review (messages, thinking, tool calls/results)
- Provide one command to run all steps sequentially
- Support running the same scenario for multiple model aliases

## Test Layout

- Integration entrypoint: `tests/e2e_steps.rs`
- Harness: `tests/e2e/harness.rs`
- Scenario modules: `tests/e2e/scenarios/<scenario>/step_XX_*.rs`

Initial scenario:

- `printer_3d`
  - `step_01_spawn_agent`
  - `step_02_run_agent_completion`
  - `step_03_reflect_agent`
  - `step_04_finalize_chat_and_reflect`

## Fixture Layout

Fixtures are committed under:

```text
tests/fixtures/e2e/<scenario>/<model_alias>/step_XX_<name>/
```

Each step stores:

- `workspace.tar.zst` (full workspace snapshot)
- `state.json` (ids, markers, previews)
- `transcript.json` (structured transcript)
- `transcript.md` (human-readable transcript)
- `metrics.json` (tool and web-fetch metrics)

## Step Chaining Rules

- Steps after `step_01` must load predecessor snapshots.
- Missing predecessor snapshot is a hard failure.
- Execution is sequential only.

## Model Matrix

Model alias is selected via:

- `GHOST_E2E_MODEL=<alias>`

Snapshots are isolated per model path.

## Refresh and Analysis Scripts

- `scripts/e2e`: interactive launcher (picker for all e2e scripts)
- `scripts/e2e/launcher.py`: questionary picker used by the launcher
- `scripts/e2e/refresh.py`: manual fixture refresh, sequential run
- `scripts/e2e/render_log.py`: render transcript JSON to markdown
- `scripts/e2e/diff.py`: compare two step artifact directories
- `scripts/e2e/analyze_request.py`: inspect raw provider debug requests

Run refresh:

```sh
uv run scripts/e2e refresh --models primary,openai
```

## Non-Goals

- Automatic fixture rewrite during normal test runs
- Parallel execution for step-based e2e

## Deferred Scenarios

- Dioxus docs import + query scenario is deferred until reference-import specs are
  implemented.
