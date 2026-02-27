# /// script
# requires-python = ">=3.11"
# dependencies = ["questionary>=2.0.1"]
# ///

"""Compare two e2e step directories.

Usage:
    uv run scripts/e2e/diff.py
    uv run scripts/e2e/diff.py --left <step_dir_a> --right <step_dir_b>
"""

from __future__ import annotations

import argparse
from pathlib import Path

import questionary

try:
    from scripts.e2e._common import list_step_dirs, read_json, short, step_label
except ModuleNotFoundError:
    from _common import list_step_dirs, read_json, short, step_label


def _tool_sequence(transcript: dict) -> list[str]:
    names: list[str] = []
    for section in ("chat", "agent"):
        for msg in transcript.get(section, []):
            for call in msg.get("tool_calls") or []:
                names.append(call.get("name", "?"))
    return names


def _final_response(state: dict) -> str:
    marker = state.get("assertion_markers", {}).get("final_response")
    if isinstance(marker, str):
        return marker
    preview = state.get("final_response_preview")
    return preview if isinstance(preview, str) else ""


def pick_step(prompt: str, exclude: Path | None = None) -> Path:
    step_dirs = list_step_dirs()
    if exclude is not None:
        step_dirs = [s for s in step_dirs if s.path != exclude]

    if not step_dirs:
        raise SystemExit("Not enough step fixtures found under tests/fixtures/e2e/")

    choices = [step_label(s) for s in step_dirs]
    selected = questionary.select(prompt, choices=choices).ask()
    if not selected:
        raise SystemExit("No step selected")

    idx = choices.index(selected)
    return step_dirs[idx].path


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Diff two e2e step artifacts")
    parser.add_argument("--left", type=Path, help="Left step directory")
    parser.add_argument("--right", type=Path, help="Right step directory")
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> None:
    args = parse_args(argv)
    left = args.left or pick_step("Select LEFT step (newest first)")
    right = args.right or pick_step("Select RIGHT step (newest first)", exclude=left)

    left_state = read_json(left / "state.json")
    right_state = read_json(right / "state.json")
    left_metrics = read_json(left / "metrics.json")
    right_metrics = read_json(right / "metrics.json")
    left_transcript = read_json(left / "transcript.json")
    right_transcript = read_json(right / "transcript.json")

    left_tools = _tool_sequence(left_transcript)
    right_tools = _tool_sequence(right_transcript)

    print("=== E2E Diff ===")
    print(f"left:  {left}")
    print(f"right: {right}")
    print()

    print("-- State --")
    print(f"left step:  {left_state.get('step')}")
    print(f"right step: {right_state.get('step')}")
    print(f"left model:  {left_state.get('model_alias')}")
    print(f"right model: {right_state.get('model_alias')}")
    print()

    print("-- Tool Sequence --")
    print(f"left ({len(left_tools)}):  {left_tools}")
    print(f"right ({len(right_tools)}): {right_tools}")
    print()

    print("-- Metrics --")
    print(
        "left web_fetch:",
        left_metrics.get("agent_web_fetch_count"),
        "urls:",
        len(left_metrics.get("agent_web_fetch_urls") or []),
    )
    print(
        "right web_fetch:",
        right_metrics.get("agent_web_fetch_count"),
        "urls:",
        len(right_metrics.get("agent_web_fetch_urls") or []),
    )
    print()

    print("-- Final Response --")
    left_final = _final_response(left_state)
    right_final = _final_response(right_state)
    print(f"left len:  {len(left_final)}")
    print(f"right len: {len(right_final)}")
    print(f"left preview:  {short(left_final)}")
    print(f"right preview: {short(right_final)}")
    if left_final != right_final:
        print("responses differ")


if __name__ == "__main__":
    main()
