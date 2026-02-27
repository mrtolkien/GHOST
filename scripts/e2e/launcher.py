# /// script
# requires-python = ">=3.11"
# dependencies = ["questionary>=2.0.1"]
# ///

"""Questionary-powered picker for e2e scripts."""

from __future__ import annotations

import subprocess
from pathlib import Path

import questionary

SCRIPT_DIR = Path(__file__).resolve().parent


def run_script(script_name: str) -> None:
    cmd = ["uv", "run", str(SCRIPT_DIR / script_name)]
    subprocess.run(cmd, check=True)


def main() -> None:
    options = [
        ("Refresh fixtures", "refresh.py"),
        ("Render transcript markdown", "render_log.py"),
        ("Diff two fixture steps", "diff.py"),
        ("Analyze debug request JSON", "analyze_request.py"),
    ]

    label = questionary.select(
        "E2E tools",
        choices=[name for name, _ in options],
    ).ask()
    if not label:
        raise SystemExit("No action selected")

    script_name = dict(options)[label]
    run_script(script_name)


if __name__ == "__main__":
    main()
