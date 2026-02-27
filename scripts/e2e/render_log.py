# /// script
# requires-python = ">=3.11"
# dependencies = ["questionary>=2.0.1"]
# ///

"""Render an e2e transcript JSON into readable markdown.

Usage:
    uv run scripts/e2e/render_log.py
    uv run scripts/e2e/render_log.py --step-dir tests/fixtures/e2e/.../step_02_run_agent_completion
"""

from __future__ import annotations

import argparse
import json
import shutil
import subprocess
from pathlib import Path

import questionary

try:
    from scripts.e2e._common import list_step_dirs, step_label
except ModuleNotFoundError:
    from _common import list_step_dirs, step_label


def _write_message(out: list[str], msg: dict, index: int) -> None:
    role = msg.get("role", "?")
    out.append(f"### {index}. {role}")
    out.append("")

    content = (msg.get("content") or "").strip()
    if content:
        out.append("**Content**")
        out.append("")
        out.append(content)
        out.append("")

    raw_output = msg.get("raw_output") or []
    if raw_output:
        out.append("**Thinking / Raw Output**")
        out.append("")
        for item in raw_output:
            ty = item.get("original_type", "unknown")
            summary = item.get("summary", "")
            out.append(f"- `{ty}`: {summary}")
        out.append("")

    calls = msg.get("tool_calls") or []
    if calls:
        out.append("**Tool Calls**")
        out.append("")
        for call in calls:
            out.append(f"- `{call.get('name', '?')}`")
            out.append("```json")
            out.append(json.dumps(call.get("input"), indent=2, ensure_ascii=False))
            out.append("```")
        out.append("")

    results = msg.get("tool_results") or []
    if results:
        out.append("**Tool Results**")
        out.append("")
        for result in results:
            out.append(f"- error={result.get('is_error', False)}")
            out.append("```text")
            out.append((result.get("content") or "").strip())
            out.append("```")
        out.append("")


def render(transcript: dict) -> str:
    out: list[str] = ["# E2E Transcript", ""]

    for section in ("chat", "agent"):
        out.append(f"## {section.capitalize()}")
        out.append("")
        messages = transcript.get(section) or []
        if not messages:
            out.append("_No messages_")
            out.append("")
            continue
        for i, msg in enumerate(messages, start=1):
            _write_message(out, msg, i)

    return "\n".join(out).strip() + "\n"


def pick_step_dir() -> Path:
    step_dirs = list_step_dirs()
    if not step_dirs:
        raise SystemExit("No step fixtures found under tests/fixtures/e2e/")

    choices = [f"{step_label(s)}" for s in step_dirs]
    selected = questionary.select(
        "Select fixture step to render (newest first)",
        choices=choices,
    ).ask()
    if not selected:
        raise SystemExit("No step selected")

    idx = choices.index(selected)
    return step_dirs[idx].path


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Render e2e transcript JSON as markdown")
    parser.add_argument("--input", type=Path, help="Path to transcript.json")
    parser.add_argument("--step-dir", type=Path, help="Step directory containing transcript.json")
    parser.add_argument("--output", type=Path, help="Output markdown path")
    parser.add_argument(
        "--no-open",
        action="store_true",
        help="Do not open the rendered transcript in a pager",
    )
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> None:
    args = parse_args(argv)

    transcript_path: Path | None = args.input
    step_dir: Path | None = args.step_dir

    if transcript_path is None and step_dir is None:
        step_dir = pick_step_dir()

    if step_dir is not None:
        transcript_path = step_dir / "transcript.json"

    if transcript_path is None or not transcript_path.exists():
        raise SystemExit(f"Transcript not found: {transcript_path}")

    data = json.loads(transcript_path.read_text())
    rendered = render(data)

    out_path = args.output
    if out_path is None and step_dir is not None:
        out_path = step_dir / "transcript.md"

    if out_path is None:
        print(rendered)
        return

    out_path.write_text(rendered)
    print(out_path)
    if not args.no_open:
        open_in_pager(out_path)


def open_in_pager(path: Path) -> None:
    bat = shutil.which("bat") or shutil.which("batcat")
    if bat:
        subprocess.run([bat, "--paging=always", str(path)], check=False)
        return

    less = shutil.which("less")
    if less:
        subprocess.run([less, "-R", str(path)], check=False)
        return

    cat = shutil.which("cat")
    if cat:
        subprocess.run([cat, str(path)], check=False)


if __name__ == "__main__":
    main()
