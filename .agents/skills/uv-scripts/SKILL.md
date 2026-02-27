---
name: uv-scripts
description: >-
  Writing and running Python code with uv inline scripts. MUST READ every time you need
  to run Python code, execute multi-line shell logic, or build a one-off data processing
  pipeline. Covers: inline dependency declarations, script placement, discoverability,
  and why scripts beat giant shell one-liners.
---

# Python Scripts with uv (NON-NEGOTIABLE)

**When you need to run Python code or complex multi-line logic, write a script file —
never paste 50+ lines into a shell command.** Scripts are reviewable, editable, and
rerunnable. Shell one-liners are none of those things.

## Before Writing a New Script

Check what already exists:

```bash
ls scripts/         # top-level scripts
ls scripts/e2e/     # e2e-specific tooling
ls scripts/tmp/     # throwaway scripts
```

If an existing script does what you need (or nearly does), extend it or reuse it instead
of writing a new one.

## Where to Put Scripts

| Kind                    | Location                                           | When                                      |
| ----------------------- | -------------------------------------------------- | ----------------------------------------- |
| Permanent utility       | `scripts/<name>.py` or `scripts/<topic>/<name>.py` | Reusable tooling the team will use again  |
| Throwaway / exploration | `scripts/tmp/<name>.py`                            | One-off analysis, debugging, data munging |

Create `scripts/tmp/` if it doesn't exist. It is gitignored.

## Inline Dependency Metadata (PEP 723)

Every script **must** declare its dependencies using the inline metadata block so
`uv run` auto-installs them with zero setup:

```python
# /// script
# requires-python = ">=3.11"
# dependencies = [
#   "requests>=2.31",
#   "rich",
# ]
# ///
```

- The `dependencies` field is **required** even if empty (`dependencies = []`).
- Pin only when necessary (`requests>=2.31`), prefer loose bounds.
- `uv run script.py` handles environment creation automatically — no venv management
  needed.

### Adding Dependencies After the Fact

```bash
uv add --script scripts/my_tool.py 'pandas>=2'
```

## Script Discoverability

Every script must start with the inline metadata block followed by a module docstring
that explains **what it does** and **how to run it**:

```python
# /// script
# requires-python = ">=3.11"
# dependencies = ["rich"]
# ///

"""One-line summary of what this script does.

Usage:
    uv run scripts/my_tool.py [--flag]
    uv run scripts/my_tool.py --help

Examples:
    uv run scripts/my_tool.py --input data.json
"""
```

This lets anyone discover the script's purpose with a quick `head` or file browse, and
`--help` via argparse gives them the rest.

## Running Scripts

```bash
uv run scripts/my_tool.py           # run with auto-installed deps
uv run scripts/tmp/explore.py       # throwaway script, same mechanism
```

## Quick Reference

- `uv run <script>` — run with auto-installed deps
- `uv add --script <script> '<pkg>'` — add a dependency to inline metadata
- `uv lock --script <script>` — create a lockfile for reproducibility
