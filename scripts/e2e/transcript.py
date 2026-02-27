# /// script
# requires-python = ">=3.11"
# dependencies = ["questionary>=2.0.1"]
# ///

"""Extract agent transcript (searches, fetches, TODOs, report) from debug request JSONs.

Usage:
    uv run scripts/e2e/transcript.py                           # interactive picker
    uv run scripts/e2e/transcript.py <e2e-output-dir-name>     # specific run
    uv run scripts/e2e/transcript.py --session <prefix>        # filter by session
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path

import questionary

try:
    from scripts.e2e._common import E2E_OUTPUT_ROOT
except ModuleNotFoundError:
    from _common import E2E_OUTPUT_ROOT


def parse_sse_response(raw: str) -> dict:
    """Parse SSE text to extract the final response object."""
    final = {}
    for line in raw.split("\n"):
        if not line.startswith("data: "):
            continue
        try:
            event = json.loads(line[6:])
        except (json.JSONDecodeError, TypeError):
            continue
        etype = event.get("type", "")
        if etype in ("response.completed", "response.incomplete"):
            final = event.get("response", {})
    return final


def extract_output(data: dict) -> list[dict]:
    """Get output items from a debug request entry."""
    response = data.get("response", {})
    if isinstance(response, str):
        parsed = parse_sse_response(response)
        return parsed.get("output", [])
    elif isinstance(response, dict):
        return response.get("output", [])
    return []


def extract_input_tool_results(data: dict) -> dict[str, str]:
    """Map call_id -> result content from function_call_output in request input."""
    results = {}
    request = data.get("request", {})
    for item in request.get("input", []):
        if item.get("type") == "function_call_output":
            call_id = item.get("call_id", "")
            output = item.get("output", "")
            results[call_id] = output
    return results


def render_turn(data: dict, turn_num: int) -> list[str]:
    """Render a single debug request turn."""
    lines: list[str] = []
    iteration = data.get("iteration", "?")
    duration = data.get("duration_ms", 0)

    lines.append(f"### Turn {turn_num} (iter={iteration}, {duration}ms)")
    lines.append("")

    output_items = extract_output(data)

    # Extract tool calls and text from output
    calls = []
    text_parts = []
    for item in output_items:
        itype = item.get("type", "")
        if itype == "function_call":
            name = item.get("name", "?")
            args_str = item.get("arguments", "{}")
            try:
                args = json.loads(args_str)
            except (json.JSONDecodeError, TypeError):
                args = {"_raw": args_str}
            calls.append({"name": name, "args": args, "call_id": item.get("call_id", "")})
        elif itype == "message":
            for content in item.get("content", []):
                if content.get("type") == "output_text":
                    text_parts.append(content.get("text", ""))

    # Also show search/fetch results from previous turn fed back as input
    tool_results = extract_input_tool_results(data)
    if tool_results:
        for call_id, result in tool_results.items():
            preview = result[:500].replace("\n", " ") + ("..." if len(result) > 500 else "")
            lines.append(f"  _Result for {call_id[:12]}: {preview}_")
            lines.append("")

    for call in calls:
        name = call["name"]
        args = call["args"]

        if name == "web_search":
            query = args.get("query", "?")
            max_r = args.get("max_results", "default")
            lines.append(f"  **SEARCH** `{query}` (max_results={max_r})")
        elif name == "web_fetch":
            url = args.get("url", "?")
            readability = args.get("readability", False)
            r_flag = " [readability]" if readability else ""
            lines.append(f"  **FETCH** {url}{r_flag}")
        elif name == "todo":
            action = args.get("action", "?")
            if action == "plan":
                items = args.get("items", [])
                lines.append(f"  **TODO plan** ({len(items)} items)")
                for i, item in enumerate(items, 1):
                    title = item.get("title", "?")
                    lines.append(f"    {i}. {title}")
            elif action == "add":
                desc = args.get("description", args.get("title", "?"))
                lines.append(f"  **TODO add** {desc}")
            elif action == "update":
                idx = args.get("index", "?")
                status = args.get("status", "?")
                lines.append(f"  **TODO update** #{idx} -> {status}")
            elif action == "batch_update":
                updates = args.get("updates", [])
                lines.append(f"  **TODO batch_update** ({len(updates)} items)")
                for u in updates:
                    lines.append(f"    #{u.get('index', '?')} -> {u.get('status', '?')}")
            else:
                lines.append(f"  **TODO {action}**")
        elif name == "knowledge_search":
            query = args.get("query", "?")
            lines.append(f"  **KNOWLEDGE** `{query}`")
        else:
            lines.append(f"  **{name}** {json.dumps(args)[:200]}")

        lines.append("")

    text = "\n".join(text_parts)
    if text:
        lines.append(f"  **FINAL REPORT** ({len(text)} chars)")
        lines.append("")
        if len(text) > 1200:
            lines.append(text[:600])
            lines.append("\n  [...truncated...]\n")
            lines.append(text[-600:])
        else:
            lines.append(text)
        lines.append("")

    return lines


def analyze_run(run_dir: Path, session_filter: str | None = None) -> str:
    req_dir = run_dir / "debug" / "requests"
    if not req_dir.exists():
        return f"No debug/requests dir in {run_dir}"

    files = sorted(req_dir.glob("*.json"), key=lambda p: p.name)
    if not files:
        return "No request JSON files found"

    # Group by session
    sessions: dict[str, list[Path]] = {}
    for f in files:
        parts = f.stem.split("_")
        if len(parts) >= 2:
            sid = parts[1]
            sessions.setdefault(sid, []).append(f)

    lines: list[str] = [f"# Transcript: {run_dir.name}", ""]
    lines.append(f"Sessions: {list(sessions.keys())}")
    lines.append("")

    for sid, sfiles in sessions.items():
        if session_filter and session_filter not in sid:
            continue

        lines.append(f"## Session {sid} ({len(sfiles)} turns)")
        lines.append("")

        for f in sfiles:
            data = json.loads(f.read_text())
            turn = f.stem.split("_")[-1] if "_" in f.stem else "?"
            lines.extend(render_turn(data, int(turn)))

    return "\n".join(lines)


def pick_run_dir() -> Path:
    if not E2E_OUTPUT_ROOT.exists():
        raise SystemExit(f"No e2e-output dir: {E2E_OUTPUT_ROOT}")

    run_dirs = sorted(
        [d for d in E2E_OUTPUT_ROOT.iterdir() if d.is_dir()],
        key=lambda p: p.stat().st_mtime,
        reverse=True,
    )
    if not run_dirs:
        raise SystemExit("No e2e-output runs found")

    choice = questionary.select(
        "Select e2e-output run (newest first)",
        choices=[d.name for d in run_dirs],
    ).ask()
    if not choice:
        raise SystemExit("No run selected")

    return E2E_OUTPUT_ROOT / choice


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Extract agent transcript from debug requests")
    parser.add_argument("run", nargs="?", help="e2e-output directory name or path")
    parser.add_argument("--session", help="Filter to session ID prefix")
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> None:
    args = parse_args(argv)

    if args.run:
        run_dir = Path(args.run)
        if not run_dir.exists():
            run_dir = E2E_OUTPUT_ROOT / args.run
        if not run_dir.exists():
            raise SystemExit(f"Not found: {run_dir}")
    else:
        run_dir = pick_run_dir()

    result = analyze_run(run_dir, session_filter=args.session)
    print(result)


if __name__ == "__main__":
    main()
