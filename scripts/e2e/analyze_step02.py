# /// script
# requires-python = ">=3.11"
# dependencies = ["rich"]
# ///
"""Analyze step_02 agent transcript from debug request files.

Usage:
    uv run scripts/e2e/analyze_step02.py [e2e-output-dir]

Defaults to the latest printer_3d_step_02 output directory.
"""

import json
import sys
from pathlib import Path

from rich.console import Console
from rich.panel import Panel

console = Console(width=120)


def find_latest_step02_dir() -> Path:
    base = Path("e2e-output")
    dirs = sorted(base.glob("*_printer_3d_step_02_*"), reverse=True)
    if not dirs:
        console.print("[red]No step_02 output directories found[/red]")
        sys.exit(1)
    return dirs[0]


def parse_sse_response(response_raw: str) -> dict:
    """Parse SSE response, return structured info."""
    tool_calls = []
    text_parts = []
    usage = {}
    status = "?"
    incomplete_reason = ""

    for line in response_raw.split("\n"):
        line = line.strip()
        if not line.startswith("data: "):
            continue
        try:
            data = json.loads(line[6:])
        except json.JSONDecodeError:
            continue

        event_type = data.get("type", "")

        if event_type == "response.output_text.delta":
            text_parts.append(data.get("delta", ""))

        elif event_type == "response.output_item.done":
            item = data.get("item", {})
            if item.get("type") == "function_call":
                name = item.get("name", "?")
                try:
                    args = json.loads(item.get("arguments", "{}"))
                except Exception:
                    args = {}
                tool_calls.append({"name": name, "args": args})

        elif event_type in ("response.completed", "response.incomplete"):
            resp = data.get("response", {})
            status = resp.get("status", "?")
            usage = resp.get("usage", {})
            if event_type == "response.incomplete":
                incomplete_reason = (
                    resp.get("incomplete_details", {}).get("reason", "?")
                )

    return {
        "tool_calls": tool_calls,
        "text": "".join(text_parts),
        "usage": usage,
        "status": status,
        "incomplete_reason": incomplete_reason,
    }


def format_tool_call(tc: dict) -> str:
    name = tc["name"]
    args = tc["args"]
    if name == "web_search":
        return f"web_search({args.get('query', '?')})"
    elif name == "web_fetch":
        return f"web_fetch({args.get('url', '?')})"
    elif name == "todo":
        action = args.get("action", "?")
        items = args.get("items", [])
        updates = args.get("updates", [])
        if action == "plan":
            titles = [i.get("title", "?") for i in items]
            return f"todo(plan, {len(items)} items: {titles})"
        elif updates:
            return f"todo({action}, {len(updates)} updates)"
        else:
            return f"todo({action}, idx={args.get('index', '?')}, status={args.get('status', '?')})"
    elif name == "knowledge_search":
        return f"knowledge_search({args.get('query', '?')[:60]})"
    else:
        return f"{name}({json.dumps(args)[:200]})"


def extract_nudges(inp: list) -> list[str]:
    """Find developer/system nudges in input."""
    nudges = []
    for msg in inp:
        if not isinstance(msg, dict):
            continue
        role = msg.get("role", "")
        if role not in ("developer", "system"):
            continue
        content = msg.get("content", "")
        if isinstance(content, list):
            for c in content:
                if isinstance(c, dict) and c.get("type") == "input_text":
                    text = c.get("text", "")
                    if text and len(text) > 20:
                        nudges.append(text[:400])
        elif isinstance(content, str) and len(content) > 20:
            nudges.append(content[:400])
    return nudges


def check_p2s_in_input(inp: list) -> list[str]:
    """Find P2S mentions in tool results."""
    findings = []
    for msg in inp:
        if not isinstance(msg, dict):
            continue
        msg_type = msg.get("type", "")
        if msg_type == "function_call_output":
            output = msg.get("output", "")
            if isinstance(output, str) and "p2s" in output.lower():
                idx = output.lower().find("p2s")
                start = max(0, idx - 80)
                end = min(len(output), idx + 80)
                findings.append(f"...{output[start:end]}...")
    return findings


def main():
    if len(sys.argv) > 1:
        output_dir = Path(sys.argv[1])
    else:
        output_dir = find_latest_step02_dir()

    base = output_dir / "debug" / "requests"
    if not base.exists():
        console.print(f"[red]Not found: {base}[/red]")
        sys.exit(1)

    console.print(f"\n[bold]Analyzing: {output_dir.name}[/bold]\n")

    all_agent = sorted(base.glob("*_H0J7QAVQ_*.json"))
    # Separate step_01 (13xxxx) from step_02 (later timestamps) by checking
    # for the continue_task user message pattern
    step02_files = []
    seen_continue = False
    for f in all_agent:
        data = json.loads(f.read_text())
        if not seen_continue:
            inp = data.get("request", {}).get("input", [])
            for msg in inp:
                if isinstance(msg, dict):
                    content = msg.get("content", "")
                    if isinstance(content, list):
                        for c in content:
                            if isinstance(c, dict) and "continue" in str(
                                c.get("text", "")
                            ).lower():
                                seen_continue = True
                                break
                    elif isinstance(content, str) and "continue" in content.lower():
                        seen_continue = True
        if seen_continue:
            step02_files.append(f)

    if not step02_files:
        step02_files = all_agent

    console.print(f"[dim]Step_02 iterations: {len(step02_files)}[/dim]\n")

    total_in = 0
    total_out = 0
    all_tool_calls = []
    p2s_sightings = []

    for f in step02_files:
        data = json.loads(f.read_text())
        iteration = data.get("iteration", "?")
        duration = data.get("duration_ms", 0)
        status_code = data.get("status", "?")
        req = data.get("request", {})
        inp = req.get("input", [])
        msg_count = len(inp)

        nudges = extract_nudges(inp)
        p2s = check_p2s_in_input(inp)
        if p2s:
            p2s_sightings.extend([(iteration, s) for s in p2s])

        resp_raw = data.get("response", "")
        if isinstance(resp_raw, str):
            parsed = parse_sse_response(resp_raw)
        else:
            parsed = {"tool_calls": [], "text": "", "usage": {}, "status": "?"}

        usage = parsed["usage"]
        total_in += usage.get("input_tokens", 0)
        total_out += usage.get("output_tokens", 0)

        # Format output
        tcs = parsed["tool_calls"]
        all_tool_calls.extend(tcs)
        tc_str = ", ".join(format_tool_call(tc) for tc in tcs)
        text = parsed["text"]

        line = (
            f"[cyan]Iter {iteration:>2}[/cyan] | {duration:>5}ms | "
            f"{msg_count:>3} msgs | in={usage.get('input_tokens', '?'):>6} "
            f"out={usage.get('output_tokens', '?'):>5}"
        )

        if text:
            line += f" | [bold yellow]TEXT ({len(text)} chars)[/bold yellow]"
        if tc_str:
            line += f" | [green]{tc_str}[/green]"

        console.print(line)

        for nudge in nudges:
            # Clean up nudge display
            if "<" in nudge:
                # Extract inner text from XML tags
                import re

                inner = re.sub(r"<[^>]+>", "", nudge).strip()
                if inner:
                    nudge = inner
            console.print(f"  [bold red]NUDGE: {nudge[:200]}[/bold red]")

        if p2s:
            for s in p2s:
                console.print(f"  [bold magenta]P2S in input: {s[:150]}[/bold magenta]")

    # Summary
    console.print(f"\n{'='*80}")
    console.print(f"[bold]Summary[/bold]")
    console.print(f"  Iterations: {len(step02_files)}")
    console.print(f"  Total tokens: in={total_in:,} out={total_out:,}")

    search_n = sum(1 for t in all_tool_calls if t["name"] == "web_search")
    fetch_n = sum(1 for t in all_tool_calls if t["name"] == "web_fetch")
    todo_n = sum(1 for t in all_tool_calls if t["name"] == "todo")
    console.print(
        f"  Tools: {len(all_tool_calls)} total "
        f"(search={search_n}, fetch={fetch_n}, todo={todo_n})"
    )

    if p2s_sightings:
        console.print(f"\n[bold magenta]P2S appeared in input at iterations: "
                      f"{[s[0] for s in p2s_sightings]}[/bold magenta]")
    else:
        console.print(f"\n[bold red]P2S never appeared in any input![/bold red]")

    # Show fetched URLs
    fetch_urls = [
        tc["args"].get("url", "?")
        for tc in all_tool_calls
        if tc["name"] == "web_fetch"
    ]
    if fetch_urls:
        console.print(f"\n[bold]Fetched URLs ({len(fetch_urls)}):[/bold]")
        for url in fetch_urls:
            console.print(f"  {url}")


if __name__ == "__main__":
    main()
