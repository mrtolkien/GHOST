Implement automated skill description optimization for GHOST.

The vendored Anthropic `skill-creator` skill includes a description optimization
pipeline that programmatically tests whether a skill triggers for various queries, then
uses an LLM to iteratively improve the description. We bundled the skill writing guide
and eval viewer but not the automation scripts, because they depend on `claude -p`
(Claude Code's headless CLI mode).

## What the upstream pipeline does

1. Takes 20 eval queries (half should-trigger, half shouldn't)
2. For each query, spawns a headless session with only the skill's name+description
   visible, and checks whether the model's first action is to invoke the skill
3. Collects pass/fail results, calls an LLM to propose an improved description
4. Re-evaluates, iterates up to 5 times with 60/40 train/test split
5. Returns the best-scoring description

## What GHOST needs

- A `ghost eval-skill` CLI command (or similar) that:
  - Starts a session with a single skill loaded
  - Sends a query
  - Returns whether the skill was triggered (and optionally the full response)
  - Runs headlessly with structured output (JSON)
- Port `run_eval.py` to call `ghost eval-skill` instead of `claude -p`
- Port `improve_description.py` to call the GHOST provider API directly (or use
  `ghost eval-skill` with a meta-prompt)
- Port `run_loop.py` to orchestrate the above
- Port `generate_report.py` (already bundled, just needs the loop output)

## Upstream reference

The original scripts are vendored at `vendor/anthropic-skills/skill-creator/scripts/`:

- `run_eval.py` — spawns parallel `claude -p` subprocesses, parses stream JSON
- `run_loop.py` — eval+improve loop with train/test split
- `improve_description.py` — calls `claude -p` to generate improved descriptions
- `generate_report.py` — HTML report from loop output (already bundled)
- `utils.py` — SKILL.md frontmatter parser (already bundled as quick_validate.py)

Sync upstream changes with `uv run scripts/sync-vendor.py anthropic-skills`.
