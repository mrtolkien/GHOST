# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

"""Detailed turn-by-turn analysis of a deep research e2e run.

Shows each turn with tool calls, content sizes, nudge messages,
and identifies where the agent went wrong.

Usage:
    uv run scripts/analyze-e2e-detail.py                     # latest run
    uv run scripts/analyze-e2e-detail.py --dir <dirname>     # specific run
"""

import argparse
import json
import sys
from pathlib import Path


def analyze_detail(run_dir: Path):
    diag = run_dir / "diagnostic.json"
    if not diag.exists():
        print(f"No diagnostic.json in {run_dir}", file=sys.stderr)
        sys.exit(1)

    with open(diag) as f:
        data = json.load(f)

    agent = data.get("agent", [])
    if not agent:
        print("No agent messages in diagnostic", file=sys.stderr)
        sys.exit(1)

    print(f"Run: {run_dir.name}")
    print(f"Total messages: {len(agent)}")
    print()

    total_content_chars = 0
    web_fetch_count = 0
    turn_num = 0

    for i, msg in enumerate(agent):
        role = msg.get("role", "?")
        content = msg.get("content", "")
        calls = msg.get("tool_calls", [])
        results = msg.get("tool_results", [])

        if role == "user" and i == 0:
            print(f"[{i}] USER prompt ({len(content)} chars)")
            print(f"     {content[:120]}...")
            total_content_chars += len(content)
            print()
            continue

        if role == "assistant":
            turn_num += 1
            call_names = [c.get("name", "?") for c in calls]

            if not content and not calls:
                print(f"[{i}] ASSISTANT turn {turn_num}: *** EMPTY RESPONSE ***")
            elif calls:
                fetch_calls = [c for c in calls if c.get("name") == "web_fetch"]
                web_fetch_count += len(fetch_calls)
                fetch_urls = []
                for c in fetch_calls:
                    url = (c.get("input") or {}).get("url", "")
                    fetch_urls.append(url)

                print(f"[{i}] ASSISTANT turn {turn_num}: {call_names}")
                if content:
                    total_content_chars += len(content)
                    print(f"     text: {content[:150]}...")
                for url in fetch_urls:
                    print(f"     fetch: {url}")
            else:
                total_content_chars += len(content)
                # Final report
                findings_lower = content.lower()
                has_p2s = "p2s" in findings_lower
                print(f"[{i}] ASSISTANT turn {turn_num}: FINAL REPORT "
                      f"({len(content)} chars, P2S={'yes' if has_p2s else 'NO'})")
                # Show first few lines
                lines = content.split("\n")[:5]
                for line in lines:
                    print(f"     {line[:100]}")

            print()
            continue

        if role == "user" and results:
            result_sizes = []
            errors = []
            for r in results:
                rc = r.get("content", "")
                total_content_chars += len(rc)
                is_err = r.get("is_error", False)
                result_sizes.append(len(rc))
                if is_err:
                    errors.append(rc[:150])

            print(f"[{i}] TOOL RESULTS: {len(results)} results, "
                  f"sizes={result_sizes}, "
                  f"total context ~{total_content_chars // 1000}K chars")
            for err in errors:
                print(f"     ERROR: {err}")
            print()
            continue

        if role == "system":
            total_content_chars += len(content)
            # Identify nudge type
            if "system-reminder" in content:
                if "empty response" in content.lower():
                    nudge_type = "RECOVERY NUDGE"
                elif "REJECTED" in content:
                    nudge_type = "PROGRESS GATE"
                elif "haven't fetched" in content:
                    nudge_type = "RECENCY NUDGE"
                elif "context window" in content:
                    nudge_type = "CONTEXT PRESSURE"
                elif "minutes" in content and "working" in content:
                    nudge_type = "TEMPORAL NUDGE"
                elif "progress" in content.lower():
                    nudge_type = "PROGRESS NUDGE"
                else:
                    nudge_type = "SYSTEM REMINDER"
            elif "<todo>" in content.lower() or "TODO" in content:
                nudge_type = "TODO INJECTION"
            else:
                nudge_type = "SYSTEM"
            print(f"[{i}] {nudge_type}: {content[:120]}...")
            print()
            continue

        # Fallback
        preview = content[:80] if content else "(empty)"
        print(f"[{i}] {role}: {preview}")
        print()

    # Summary
    print("=" * 60)
    print(f"Turns: {turn_num}, web_fetch: {web_fetch_count}, "
          f"total context: ~{total_content_chars // 1000}K chars")

    # Check test assertions
    test_domains = ["all3dp.com", "auroratechchannel.com"]
    fetched_urls = []
    for msg in agent:
        for c in msg.get("tool_calls", []):
            if c.get("name") == "web_fetch":
                fetched_urls.append((c.get("input") or {}).get("url", ""))

    domain_match = any(d in u for d in test_domains for u in fetched_urls)

    # Find findings
    findings = ""
    for msg in reversed(agent):
        if (msg.get("role") == "assistant"
                and msg.get("content")
                and len(msg["content"]) > 200
                and not msg.get("tool_calls")):
            findings = msg["content"]
            break

    has_p2s = "p2s" in findings.lower() if findings else False

    print(f"\nTest assertion check:")
    print(f"  findings > 200 chars: {'PASS' if len(findings) > 200 else 'FAIL'} ({len(findings)})")
    print(f"  web_fetch >= 5: {'PASS' if web_fetch_count >= 5 else 'FAIL'} ({web_fetch_count})")
    print(f"  domain match: {'PASS' if domain_match else 'FAIL'} ({test_domains})")
    print(f"  P2S in findings: {'PASS' if has_p2s else 'FAIL'}")

    all_pass = (len(findings) > 200 and web_fetch_count >= 5
                and domain_match and has_p2s)
    print(f"\n  Overall: {'PASS' if all_pass else 'FAIL'}")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--dir", type=str, help="Specific run directory")
    args = parser.parse_args()

    e2e_dir = Path(__file__).parent.parent / "e2e-output"

    if args.dir:
        run_dir = Path(args.dir)
        if not run_dir.is_absolute():
            run_dir = e2e_dir / args.dir
    else:
        runs = sorted(
            [d for d in e2e_dir.iterdir()
             if d.is_dir() and "deep_research" in d.name],
            reverse=True,
        )
        if not runs:
            print("No deep_research runs found", file=sys.stderr)
            sys.exit(1)
        run_dir = runs[0]

    analyze_detail(run_dir)


if __name__ == "__main__":
    main()
