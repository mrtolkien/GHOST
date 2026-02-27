# /// script
# requires-python = ">=3.11"
# dependencies = ["questionary>=2.0.1"]
# ///

"""Analyze debug request JSON files from e2e-output runs."""

from __future__ import annotations

import argparse
import json
from pathlib import Path

import questionary

try:
    from scripts.e2e._common import E2E_OUTPUT_ROOT
except ModuleNotFoundError:
    from _common import E2E_OUTPUT_ROOT


def analyze_request(filepath: Path) -> None:
    data = json.loads(filepath.read_text())
    messages = data.get("messages", [])
    system = data.get("system", "")
    model = data.get("model", "?")

    print(f"File: {filepath}")
    print(f"Model: {model}")
    print(f"System prompt: {len(system)} chars")
    print(f"Messages: {len(messages)}")
    print()

    total_chars = len(system)
    for i, msg in enumerate(messages):
        role = msg.get("role", "?")
        content = msg.get("content", [])

        if isinstance(content, str):
            total_chars += len(content)
            print(f"  [{i}] {role}: {len(content)} chars")
            continue

        msg_chars = 0
        tool_uses = 0
        tool_results = 0
        for block in content:
            t = block.get("type", "")
            if t == "text":
                msg_chars += len(block.get("text", ""))
            elif t == "tool_use":
                tool_uses += 1
                msg_chars += len(json.dumps(block.get("input", {})))
            elif t == "tool_result":
                tool_results += 1
                rc = block.get("content", "")
                if isinstance(rc, list):
                    rc_text = "".join(
                        b.get("text", "") for b in rc if isinstance(b, dict)
                    )
                else:
                    rc_text = rc
                msg_chars += len(rc_text)

        total_chars += msg_chars
        print(
            f"  [{i}] {role}: {msg_chars} chars "
            f"(tool_uses={tool_uses}, tool_results={tool_results})"
        )

    print()
    print(f"Total context: {total_chars:,} chars (~{total_chars // 4:,} tokens)")


def pick_request_file() -> Path:
    if not E2E_OUTPUT_ROOT.exists():
        raise SystemExit(f"No e2e-output dir: {E2E_OUTPUT_ROOT}")

    run_dirs = sorted(
        [d for d in E2E_OUTPUT_ROOT.iterdir() if d.is_dir()],
        key=lambda p: p.stat().st_mtime,
        reverse=True,
    )
    if not run_dirs:
        raise SystemExit("No e2e-output runs found")

    run_choice = questionary.select(
        "Select e2e-output run (newest first)",
        choices=[d.name for d in run_dirs],
    ).ask()
    if not run_choice:
        raise SystemExit("No run selected")
    run_dir = E2E_OUTPUT_ROOT / run_choice

    req_dir = run_dir / "debug" / "requests"
    files = sorted(req_dir.glob("*.json"), key=lambda p: p.name)
    if not files:
        raise SystemExit(f"No request JSON files found in {req_dir}")

    file_choice = questionary.select(
        "Select request file",
        choices=[f.name for f in files],
    ).ask()
    if not file_choice:
        raise SystemExit("No request file selected")

    return req_dir / file_choice


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Analyze request JSON payloads")
    parser.add_argument("file", nargs="?", help="Direct path to request JSON")
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> None:
    args = parse_args(argv)
    if args.file:
        path = Path(args.file)
        if not path.exists():
            raise SystemExit(f"File not found: {path}")
    else:
        path = pick_request_file()

    analyze_request(path)


if __name__ == "__main__":
    main()
