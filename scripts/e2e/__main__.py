"""Interactive launcher for e2e scripts.

Usage:
    uv run scripts/e2e
"""

from __future__ import annotations

import argparse
import subprocess
import sys
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Interactive e2e script launcher")
    parser.add_argument(
        "action",
        nargs="?",
        choices=["refresh", "render-log", "diff", "analyze-request"],
        help="Optional direct action; if omitted, an interactive picker is shown.",
    )
    parser.add_argument("extra", nargs=argparse.REMAINDER, help="Arguments forwarded to action")
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> None:
    args = parse_args(argv)

    if args.action == "refresh":
        run_script("refresh.py", args.extra)
        return
    if args.action == "render-log":
        run_script("render_log.py", args.extra)
        return
    if args.action == "diff":
        run_script("diff.py", args.extra)
        return
    if args.action == "analyze-request":
        run_script("analyze_request.py", args.extra)
        return

    interactive_picker()


def run_script(script_name: str, extra: list[str]) -> None:
    cmd = ["uv", "run", str(SCRIPT_DIR / script_name), *extra]
    subprocess.run(cmd, check=True)


def interactive_picker() -> None:
    run_script("launcher.py", [])


if __name__ == "__main__":
    main(sys.argv[1:])
