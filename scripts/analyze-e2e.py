# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

"""Analyze e2e-output diagnostic JSON files for deep research test runs.

Usage:
    uv run scripts/analyze-e2e.py                    # latest 6 runs
    uv run scripts/analyze-e2e.py --count 10         # latest 10 runs
    uv run scripts/analyze-e2e.py --dir e2e-output/2026-02-26T01-47-46_deep_research
"""

import argparse
import json
import os
import sys
from pathlib import Path


def analyze_run(run_dir: Path) -> dict:
    """Extract key metrics from a single e2e run's diagnostic.json."""
    diag = run_dir / "diagnostic.json"
    if not diag.exists():
        return {"dir": run_dir.name, "error": "no diagnostic.json"}

    with open(diag) as f:
        data = json.load(f)

    agent = data.get("agent", [])
    if not agent:
        return {"dir": run_dir.name, "error": "no agent messages"}

    total_messages = len(agent)
    web_fetch_count = 0
    web_fetch_urls = []
    tool_errors = []
    tool_call_names = []
    empty_responses = 0

    for msg in agent:
        role = msg.get("role", "?")
        content = msg.get("content", "")
        calls = msg.get("tool_calls", [])
        results = msg.get("tool_results", [])

        for call in calls:
            name = call.get("name", "?")
            tool_call_names.append(name)
            if name == "web_fetch":
                web_fetch_count += 1
                url = (call.get("input") or {}).get("url", "")
                if url:
                    web_fetch_urls.append(url)

        for result in results:
            if result.get("is_error"):
                tool_errors.append(result.get("content", "")[:200])

        # Detect empty assistant responses (no content, no tool calls)
        if role == "assistant" and not content and not calls:
            empty_responses += 1

    # Check last message
    last = agent[-1]
    last_role = last.get("role", "?")
    last_content = last.get("content", "")
    last_calls = [c.get("name", "?") for c in last.get("tool_calls", [])]
    last_results = last.get("tool_results", [])

    # Extract domains from URLs
    domains = set()
    for url in web_fetch_urls:
        try:
            from urllib.parse import urlparse
            host = urlparse(url).hostname or ""
            # Strip www.
            if host.startswith("www."):
                host = host[4:]
            domains.add(host)
        except Exception:
            pass

    # Check for expected domains and keywords
    expected_domains = [
        "all3dp.com", "auroratechchannel.com", "tomshardware.com",
        "aniwaa.com", "pcmag.com", "3dwithus.com",
    ]
    matched_domains = [d for d in expected_domains if d in domains]

    # Check findings for P2S
    findings = ""
    for msg in reversed(agent):
        if msg.get("role") == "assistant" and msg.get("content") and len(msg["content"]) > 200:
            findings = msg["content"]
            break

    has_p2s = "p2s" in findings.lower() if findings else False

    return {
        "dir": run_dir.name,
        "messages": total_messages,
        "web_fetch": web_fetch_count,
        "domains": sorted(domains),
        "matched_domains": matched_domains,
        "has_p2s": has_p2s,
        "findings_len": len(findings),
        "empty_responses": empty_responses,
        "tool_errors": tool_errors,
        "last_role": last_role,
        "last_calls": last_calls if last_calls else None,
        "last_content_len": len(last_content) if last_content else 0,
    }


def print_run(info: dict):
    name = info["dir"]
    if "error" in info:
        print(f"\n{'='*60}")
        print(f"{name}")
        print(f"  ERROR: {info['error']}")
        return

    # Determine pass/fail
    passed = (
        info["web_fetch"] >= 5
        and len(info["matched_domains"]) > 0
        and info["has_p2s"]
        and info["findings_len"] > 200
    )
    status = "PASS" if passed else "FAIL"

    print(f"\n{'='*60}")
    print(f"{name}  [{status}]")
    print(f"  messages: {info['messages']}, "
          f"web_fetch: {info['web_fetch']}, "
          f"findings: {info['findings_len']} chars")
    print(f"  domains: {', '.join(info['domains'])}")
    print(f"  matched: {info['matched_domains'] or 'NONE'}")
    print(f"  P2S: {'yes' if info['has_p2s'] else 'NO'}")

    if info["empty_responses"]:
        print(f"  empty responses: {info['empty_responses']}")

    if info["tool_errors"]:
        print(f"  tool errors ({len(info['tool_errors'])}):")
        for err in info["tool_errors"][:3]:
            print(f"    - {err[:120]}")

    # Show how the session ended
    if info["last_calls"]:
        print(f"  ended with: {info['last_role']} calls={info['last_calls']}")
    elif info["last_content_len"]:
        print(f"  ended with: {info['last_role']} ({info['last_content_len']} chars)")
    else:
        print(f"  ended with: {info['last_role']} (empty)")


def main():
    parser = argparse.ArgumentParser(description="Analyze deep research e2e test outputs")
    parser.add_argument("--count", type=int, default=6, help="Number of recent runs to show")
    parser.add_argument("--dir", type=str, help="Analyze a specific run directory")
    args = parser.parse_args()

    e2e_dir = Path(__file__).parent.parent / "e2e-output"
    if not e2e_dir.exists():
        print(f"No e2e-output directory found at {e2e_dir}", file=sys.stderr)
        sys.exit(1)

    if args.dir:
        run_dir = Path(args.dir)
        if not run_dir.is_absolute():
            run_dir = e2e_dir / args.dir
        info = analyze_run(run_dir)
        print_run(info)
        return

    # Find deep_research runs, sorted by name (timestamp-based)
    runs = sorted(
        [d for d in e2e_dir.iterdir() if d.is_dir() and "deep_research" in d.name],
        reverse=True,
    )[:args.count]

    if not runs:
        print("No deep_research runs found in e2e-output/", file=sys.stderr)
        sys.exit(1)

    print(f"Analyzing {len(runs)} most recent deep_research runs:\n")

    pass_count = 0
    fail_count = 0
    for run_dir in reversed(runs):  # chronological order
        info = analyze_run(run_dir)
        print_run(info)
        if "error" not in info:
            passed = (
                info["web_fetch"] >= 5
                and len(info["matched_domains"]) > 0
                and info["has_p2s"]
                and info["findings_len"] > 200
            )
            if passed:
                pass_count += 1
            else:
                fail_count += 1

    print(f"\n{'='*60}")
    print(f"Summary: {pass_count} PASS, {fail_count} FAIL "
          f"out of {pass_count + fail_count} runs")


if __name__ == "__main__":
    main()
