---
name: e2e-testing
description: >-
  Step-based e2e test design, implementation, and iteration for Ghost. Use this skill
  when creating, rewriting, reviewing, or debugging end-to-end tests that chain fixtures
  across steps, use LiveTestEnv snapshots, run sequentially, or compare behavior across
  model aliases. Covers: tests/e2e harness layout, hard-fail predecessor fixtures,
  transcript/metrics artifacts, interactive scripts/e2e tooling, and the iteration
  workflow when tests fail.
---

# E2E Testing (Step-Based)

## When To Read This Skill

Read this skill before making changes if the task mentions any of:

- e2e tests, live integration tests, or scenario tests
- splitting one flow into multiple steps
- fixture chaining or snapshot restore between tests
- multi-model e2e runs (`GHOST_E2E_MODEL`)
- transcript/log rendering or fixture diffs
- a failing e2e test that needs debugging or iteration

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

## Iterating On Failing E2E Tests (NON-NEGOTIABLE)

When an e2e test fails, follow this workflow strictly. The goal is to fix the _agent
behavior_, never the test.

### Rule 1: Never Change Test Assertions

Assertions encode the RIGHT answers. They are strict on purpose. Do not weaken, relax,
remove, or work around them. If a test fails, the agent is wrong — not the test.

### Rule 2: Never Over-Fit Prompts To Tests

Agent prompts, skill descriptions, and agent definitions are **generic** — they must
stay generic. Do not add test-specific keywords, model names, or domain knowledge that
only exists to pass a particular assertion. A prompt change is over-fitting if it would
not make sense for a real user query outside the test scenario.

Bad: adding "always mention the Bambu Lab P2S" to the research agent prompt. Good:
improving the workflow step that tells the agent to check dedicated review sites for
recent models.

### Rule 3: Start With Prompt Engineering

When a test fails, the first thing to do — before touching any Rust code — is to
understand what the model did wrong and why. Follow this order:

1. **Read the transcript.** Use `uv run scripts/e2e render-log` to get a readable
   markdown view of the agent's conversation. Look at what the model actually said,
   which tools it called, and in what order.

2. **Check TODO items and tool calls.** The model's internal planning (TODO lists,
   reasoning traces) reveals how it interpreted the prompt. If it skipped a step, the
   prompt likely did not make that step clear enough.

3. **Read the failing assertion.** Understand exactly what the test expected and what
   was missing. Map the gap back to a specific prompt instruction or workflow step.

4. **Review the relevant prompts.** Read the agent prompt (`prompts/agents/<agent>.md`),
   any skills it loads, and the system prompt. Identify where the instruction was
   ambiguous, too weak, or missing.

5. **Make a targeted prompt edit.** Change one thing at a time. Re-run the failing step
   to see if it helped. Use `uv run scripts/e2e diff` to compare before/after
   transcripts.

Common prompt-level fixes:

- Strengthening a workflow step that the model skips
- Reordering steps so the model does not short-circuit
- Making a vague instruction concrete (without over-fitting)
- Adjusting a skill's `description` field so the model reads it at the right time

### Rule 4: Architectural Changes Are A Last Resort

Only consider code changes (tool loop, nudges, context management, handler logic) if
prompt engineering alone cannot fix the failure after multiple attempts. When you do:

1. **Isolate the hypothesis.** State exactly what you think the code-level problem is
   (e.g., "the progress gate fires too early and truncates research").

2. **Test the hypothesis.** Add logging or a small targeted change to confirm the theory
   before making a larger refactor.

3. **Keep changes minimal.** Fix the specific issue — do not refactor surrounding code,
   add new abstractions, or "improve" unrelated paths.

4. **Verify with a full run.** After a code change, re-run the entire scenario (all
   steps), not just the failing step.

### Iteration Quick Reference

```sh
# 1. Run the failing step
GHOST_E2E_MODEL=primary cargo test --features e2e-tests \
  --test e2e_steps printer_3d_step_02 -- --nocapture --test-threads=1

# 2. Read the transcript
uv run scripts/e2e render-log

# 3. Compare before/after
uv run scripts/e2e diff

# 4. Inspect raw request payloads
uv run scripts/e2e analyze-request

# 5. Re-run entire scenario after a fix
uv run scripts/e2e refresh --models primary
```

## Debugging Methodology (Systematic)

When an agent-driven e2e test fails, follow this structured diagnostic process.
Do NOT guess or make random prompt changes — diagnose first, then fix.

### Step 1: Identify the Failing Assertion

Read the panic message. Map it to the exact assertion in the test file. Know
precisely what the test expected vs what it got (e.g., "P2S missing from
findings", "findings only 49 chars", "web_fetch count < 5").

### Step 2: Analyze the Agent Transcript

Use `uv run scripts/e2e/analyze_step02.py` (or `render-log` for fixture dirs)
to get a structured view of the agent's behavior. Key things to check:

- **Tool call sequence**: Did the agent search, plan, fetch, then write? Or did
  it short-circuit?
- **TODO plan**: Was one created? How many items? Were they followed in order?
- **Fetched URLs**: Did the agent fetch specialist review sites, or only Reddit?
- **Token counts**: Are input tokens growing as expected, or stalling?
- **Final text output**: How many chars? Does it contain the expected keywords?

### Step 3: Check Whether the Data Exists in Context

Before blaming the prompt, verify whether the expected information actually
appeared in the agent's input. Use the analysis script to search for keywords
(e.g., "P2S") across all tool results. If the data IS in the input but missing
from the output, the problem is synthesis/reporting, not search. If the data is
NOT in the input, the problem is search/fetch.

### Step 4: Inspect Nudges and System Messages

Count developer/system messages per iteration. Known failure modes:

- **TODO injection stacking**: Each `post_tool_iteration` pushes TODO state.
  If it doesn't remove the previous one, copies accumulate (fixed Feb 2026 —
  `history.retain()` now removes old TODO messages before pushing new).
- **Context pressure firing too early**: The `context_pressure.threshold_chars`
  config counts characters, not tokens. A 250K char threshold fires at ~62K
  tokens — way too early for a 250K-token context window. Currently disabled
  for the deep-research agent; masking handles context management.
- **Iteration countdown stacking**: Check if countdown nudges accumulate.
- **Nudge-to-content ratio**: If developer messages exceed 50% of the final
  iteration's input, the agent is being drowned in noise.

### Step 5: Check for Infrastructure Issues

- **Provider timeouts**: Look for `timed out, retrying` in test output. Two
  consecutive timeouts kill the agent run.
- **Web fetch failures**: Empty or error tool results mean the page didn't load.
- **Masking artifacts**: If `apply_masking_if_needed` fires, old tool results
  get truncated. This is normal but can lose information the agent needs.

### Analysis Scripts

```sh
# Structured agent transcript analysis (works with Codex Responses API format)
uv run scripts/e2e/analyze_step02.py [e2e-output-dir]

# Interactive tools
uv run scripts/e2e render-log        # Readable markdown from fixture dirs
uv run scripts/e2e diff              # Compare two fixture outputs
uv run scripts/e2e analyze-request   # Inspect raw request payloads
```

### Iteration Journal

Save root-cause analyses and fixes to `e2e-output/iteration_journal.md`. One
row per resolved issue, newest first. Always update the journal when closing
an e2e debugging session so future runs start with context.

## Review Checklist

- Does each step do exactly one thing?
- Are predecessor fixtures enforced with hard-fail?
- Is state handoff explicit and minimal?
- Are transcript markdown and metrics produced?
- Can the flow run sequentially for multiple model aliases?
