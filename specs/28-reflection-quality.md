# Spec 28: Reflection Quality Test

## Context

Spec 26 defined the full e2e flow (chat → agent → reflection). The agent phase works
well. Reflection does not: it either produces zero tool calls (text-only planning) or
stops after 2-3 calls with a "corrective pass needed" handoff.

This spec focuses on getting reflection to produce **high-quality knowledge artifacts**
from a rich agent transcript. It depends on spec 27 (job–agent unification) for the
execution layer.

## Test fixture

`tests/fixtures/e2e_research_post_agent/` contains a snapshot from a successful
deep-research run about enclosed 3D printers under $1000:

- `agent_transcript.json` — 16 messages (user query + agent tool calls + findings)
- `web-cache/` — 25 cached web pages the agent fetched

This fixture is stable and reusable. The isolated test replays the transcript into a DB
session, installs the web cache, and runs reflection without any chat or agent phase.

**Important**: The current fixture was captured before the `<system-reminder><progress>`
XML format (spec 27). After spec 27 phase 1 lands, re-capture the fixture by running the
full `e2e_research` test (which goes through chat → agent → reflection) so the agent
transcript contains the new XML-format progress messages. This ensures the reflection
test exercises the same format the model will see in production.

## Test: `reflection_on_agent_transcript`

**File**: `tests/heartbeat_reflection.rs`

```
cargo test --features live-tests reflection_on_agent_transcript -- --nocapture
```

### Setup

1. Create `LiveTestEnv` with fresh temp workspace + DB
2. Replay `agent_transcript.json` into a session via `session_from_transcript()`
3. Copy `web-cache/` fixture into workspace via `install_web_cache_fixture()`

### Execution

Run `env.run_reflection(&session, None)` with a 3-minute timeout.

### Assertions (graduated)

**Tier 1 — basic function** (must pass):

- Handoff note is non-empty
- At least one note OR reference was created (reflection used tools)

**Tier 2 — entity coverage** (target):

- A note mentioning "P2S" or "p2s" exists (the agent's top finding)
- A source quality note for "all3dp" or "aurora" exists

**Tier 3 — structural quality** (aspirational):

- Notes are in subfolders (first tag → subfolder placement)
- Entity notes contain wiki links to references (`[[references/...]]`)
- Decision note exists linking entity notes
- Web cache files were curated (moved or deleted, not left untouched)

### Diagnostic output

All runs write to `e2e-output/{timestamp}_reflection_agent_transcript/`:

- `diagnostic.json` — structured: notes list, references list, handoff, P2S found,
  source note found
- `diagnostic.log` — human-readable summary

## Iteration journal

`e2e-output/reflection_iterations.md` tracks each attempt with: hypothesis, changes
made, results, and conclusions. This is the primary debugging artifact.

## Known failure modes

From attempts 1-2 (documented in the iteration journal):

1. **Text-only planning**: Model describes what it _would_ do but calls zero tools.
   **Fix applied**: CRITICAL directive in reflection.md ("text-only = failure").

2. **Early exit**: Model calls 2-3 tools then writes a handoff saying "need corrective
   pass." Stops at the first friction point (e.g., malformed filename). **Fix applied**:
   Progress tracking — `<system-reminder><progress>` XML injected after each iteration
   showing note_write and reference_manage counts so the model sees how little it's
   done. Optional nudge messages fire to keep it going.

3. **Empty EndTurn**: Provider returns empty response. Transient. Retry addresses this.

## Implementation order

Reflection quality work is blocked on spec 27 phase 1 (reflection-as-agent). The
execution path must be stable before tuning the prompt.

### Step 1: Reflection as agent (spec 27 phase 1)

- Move reflection prompt to `agents/reflection.md` with TOML frontmatter
- Progress rules, max_iterations, tools all declared in frontmatter
- `ReflectionManager` handles trigger/context, `AgentRunner` handles execution
- Isolated test updated to use `AgentRunner` path

### Step 2: Baseline with new execution path

- Run `reflection_on_agent_transcript`, record results in iteration journal
- Confirm tier 1 assertions pass with the agent execution path
- If they don't, debug the execution path (not the prompt)

### Step 3: Prompt iteration

Iterate on `agents/reflection.md` prompt until tier 2 assertions pass. Levers:

- **max_iterations**: Start at 40 (same as agents). Reflection needs room to: discover
  structure, curate 25 cache files, create 5+ notes, update diary.
- **Progress tracking**: `<system-reminder><progress>` XML shows tool counts. Nudge
  messages (optional, with optional min threshold) give the prompt engineer control over
  when and what to say. Tune nudge text and thresholds based on observed behavior.
- **Workflow simplification**: The 9-step workflow may be too many steps. Consider
  collapsing to: discover → curate → create notes → handoff.
- **Full agent context**: If the filtered transcript loses too much, try passing the
  full agent session (with tool results containing page content). This avoids the model
  needing to re-read cached files.

### Step 4: Structural quality

Once tier 2 passes, work toward tier 3:

- Verify subfolder placement (spec plan part 1 already implemented)
- Check wiki links in note bodies
- Verify web cache curation (files moved/deleted vs. left untouched)

## Success criteria

The test is considered passing when:

- Tier 1 assertions pass reliably (>90% of runs)
- Tier 2 assertions pass most of the time (>70% of runs)
- The iteration journal documents at least one clean run with tier 2 passing

## Forbidden

Per spec 26:

- Do NOT make the reflection prompt specific to 3D printers or product research
- Do NOT create low-quality hacks to pass specific assertions
- If the approach doesn't work, pursue architectural changes (spec 27) rather than
  prompt hacks
