# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""List all scripts in scripts/ with their top-level docstrings.

Usage:
    uv run scripts/discover_scripts.py
    uv run scripts/discover_scripts.py --include-tmp
"""

from __future__ import annotations

import argparse
import ast
import sys
from pathlib import Path

SCRIPTS_ROOT = Path(__file__).resolve().parent


def extract_docstring(path: Path) -> str | None:
    """Parse a Python file and return its module docstring, or None."""
    try:
        tree = ast.parse(path.read_text())
    except SyntaxError:
        return None
    return ast.get_docstring(tree)


def discover(include_tmp: bool = False) -> list[tuple[str, str | None]]:
    """Walk scripts/ and return (relative_path, docstring) pairs."""
    results: list[tuple[str, str | None]] = []

    for py in sorted(SCRIPTS_ROOT.rglob("*.py")):
        rel = py.relative_to(SCRIPTS_ROOT)

        # Skip __pycache__, __init__, __main__
        if "__pycache__" in rel.parts:
            continue
        if rel.name.startswith("__"):
            continue
        # Skip tmp/ unless asked
        if not include_tmp and rel.parts[0] == "tmp":
            continue

        docstring = extract_docstring(py)
        first_line = docstring.strip().split("\n")[0] if docstring else None
        results.append((str(rel), first_line))

    return results


def main() -> None:
    parser = argparse.ArgumentParser(description="Discover available scripts")
    parser.add_argument(
        "--include-tmp", action="store_true", help="Include scripts/tmp/"
    )
    args = parser.parse_args()

    scripts = discover(include_tmp=args.include_tmp)
    if not scripts:
        print("No scripts found.")
        sys.exit(1)

    # Find max path width for alignment
    max_path = max(len(s[0]) for s in scripts)

    print(f"{'Script':<{max_path}}  Description")
    print(f"{'─' * max_path}  {'─' * 60}")
    for path, doc in scripts:
        desc = doc or "(no docstring)"
        print(f"{path:<{max_path}}  {desc}")


if __name__ == "__main__":
    main()
