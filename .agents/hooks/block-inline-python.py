#!/usr/bin/env python3
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Claude Code PreToolUse hook: blocks inline data processing in bash.

Enforces the Script-First Execution rule from CLAUDE.md.
Blocked patterns: python3 -c, | jq, awk '<program>'.
"""
import json
import re
import sys

data = json.load(sys.stdin)
cmd = data.get("tool_input", {}).get("command", "")

violations = []
if re.search(r"\bpython3?\s+-c\b", cmd):
    violations.append("python -c")
if re.search(r"\|\s*jq\b", cmd):
    violations.append("| jq")
if re.search(r"\bawk\s+['\"]", cmd):
    violations.append("awk")

if violations:
    print(
        f"BLOCKED ({', '.join(violations)}): Write a script to scripts/ "
        f"and run with 'uv run'. Read the /uv-scripts skill.",
        file=sys.stderr,
    )
    sys.exit(1)
