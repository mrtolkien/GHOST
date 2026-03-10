---
name: scripting
description:
  Write reusable scripts. Use when the OPERATOR asks a question that requires running
  code, data processing, API calls, or anything beyond a single coreutils command.
---

# Scripting

When answering a question that requires code, write a clean, reusable script rather than
a throwaway command. This makes your work reproducible and discoverable.

## Before Writing

Check if you already have a script for this:

```
knowledge_search query="<what you need>" categories=["scripts"]
```

If found, read it with `read_file` and run it. Update it if the OPERATOR's request is a
variation.

## File Organization

Scripts live in the workspace under `scripts/{topic}/{name}.py`:

- **topic**: broad category (e.g., `finance`, `domains`, `weather`, `system`)
- **name**: descriptive, lowercase with underscores (e.g., `spending_by_category.py`)

## Python Scripts (Default)

Use Python with [uv inline metadata (PEP 723)](https://docs.astral.sh/uv/guides/scripts/).
Every script MUST have:

1. **PEP 723 metadata block** — declares dependencies inline
2. **Module docstring** — what it does, when to use it (this powers search)
3. **typer** for scripts with arguments — gives automatic `--help`

### Template (with arguments)

```python
# /// script
# requires-python = ">=3.12"
# dependencies = ["httpx", "typer"]
# ///
"""Short description of what this script does.

Longer explanation of when to use it and what it expects.
"""

import typer

def main(
    arg1: str = typer.Argument(help="Description of arg1"),
    flag: bool = typer.Option(False, help="Description of flag"),
):
    """One-line description for --help."""
    ...

if __name__ == "__main__":
    typer.run(main)
```

### Template (no arguments)

```python
# /// script
# requires-python = ">=3.12"
# dependencies = ["httpx"]
# ///
"""Short description of what this script does.

Longer explanation.
"""

def main():
    ...

if __name__ == "__main__":
    main()
```

### Running

```
run_shell_command command="uv run scripts/{topic}/{name}.py [args]"
```

## Non-Python Scripts

Use Bash only for thin wrappers around existing CLI tools with no complex logic:

```bash
#!/usr/bin/env bash
# Short description of what this script does.
set -euo pipefail

# ... implementation
```

For other languages (Go, Rust), add required toolchains to the workspace nix shell
(`shell/`) and document the build step in the script header.

## When NOT to Script

- Single coreutils command (`ls`, `grep`, `wc`, `df`, `du`) — just run it
- Quick one-liner with no dependencies — just run it

## When to Script

- Needs a Python library (parsing, HTTP, data processing)
- Has logic (conditionals, loops over data, error handling)
- Would benefit from reuse (OPERATOR might ask again)
- Needs structured output (tables, formatted reports)

## Library Documentation

If you're unsure about a library's API, import its documentation first:

```
# For PyPI packages with docs sites:
run_shell_command command="ghost reference import crawl --url https://typer.tiangolo.com/ --topic typer --max-depth 2"

# For GitHub repos:
run_shell_command command="ghost reference import git --url https://github.com/tiangolo/typer --topic typer --paths docs/ --extensions md"
```

Then search your references:

```
knowledge_search query="typer argument" categories=["references"] topic="typer"
```
