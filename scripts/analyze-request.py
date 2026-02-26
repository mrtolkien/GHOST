# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

"""Analyze raw debug request JSON files to see what the model actually received.

Shows message sizes, system messages (nudges), and total context.

Usage:
    uv run scripts/analyze-request.py <request_file>
    uv run scripts/analyze-request.py --dir <e2e_dir> --iter 6  # specific iteration
    uv run scripts/analyze-request.py --dir <e2e_dir> --last     # last request
"""

import argparse
import json
import sys
from pathlib import Path


def analyze_request(filepath: Path):
    with open(filepath) as f:
        data = json.load(f)

    messages = data.get("messages", [])
    system = data.get("system", "")
    model = data.get("model", "?")

    print(f"File: {filepath.name}")
    print(f"Model: {model}")
    print(f"System prompt: {len(system)} chars")
    print(f"Messages: {len(messages)}")
    print()

    total_chars = len(system)

    for i, msg in enumerate(messages):
        role = msg.get("role", "?")
        content = msg.get("content", [])

        if isinstance(content, str):
            # Simple string content
            total_chars += len(content)
            if role == "system":
                # Show system messages in full (these are nudges)
                if len(content) < 500:
                    print(f"  [{i}] {role}: {content}")
                else:
                    print(f"  [{i}] {role}: ({len(content)} chars) "
                          f"{content[:150]}...")
            else:
                print(f"  [{i}] {role}: {len(content)} chars")
            continue

        # Array content (tool uses, tool results, text blocks)
        msg_chars = 0
        tool_uses = []
        tool_results = []
        text_blocks = []
        masked = 0

        for block in content:
            block_type = block.get("type", "")

            if block_type == "text":
                text = block.get("text", "")
                msg_chars += len(text)
                text_blocks.append(len(text))

            elif block_type == "tool_use":
                name = block.get("name", "?")
                inp = block.get("input", {})
                inp_size = len(json.dumps(inp))
                msg_chars += inp_size
                tool_uses.append(f"{name}({inp_size})")

            elif block_type == "tool_result":
                # Content can be string or list
                rc = block.get("content", "")
                if isinstance(rc, list):
                    rc_text = "".join(
                        b.get("text", "") for b in rc
                        if isinstance(b, dict)
                    )
                else:
                    rc_text = rc
                msg_chars += len(rc_text)
                is_err = block.get("is_error", False)
                if rc_text == "[content masked]":
                    masked += 1
                else:
                    tool_results.append(
                        f"{'ERR ' if is_err else ''}{len(rc_text)}"
                    )

        total_chars += msg_chars

        parts = []
        if text_blocks:
            parts.append(f"text={text_blocks}")
        if tool_uses:
            parts.append(f"tools=[{', '.join(tool_uses)}]")
        if tool_results:
            parts.append(f"results=[{', '.join(tool_results)}]")
        if masked:
            parts.append(f"masked={masked}")

        print(f"  [{i}] {role}: {msg_chars} chars "
              f"({', '.join(parts) if parts else 'empty'})")

    print()
    print(f"Total context: {total_chars:,} chars (~{total_chars // 4:,} tokens)")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("file", nargs="?", help="Request JSON file path")
    parser.add_argument("--dir", type=str, help="E2E output directory")
    parser.add_argument("--iter", type=int, help="Iteration number")
    parser.add_argument("--last", action="store_true", help="Last request")
    args = parser.parse_args()

    if args.file:
        analyze_request(Path(args.file))
        return

    if args.dir:
        e2e_dir = Path(__file__).parent.parent / "e2e-output"
        run_dir = e2e_dir / args.dir if not Path(args.dir).is_absolute() \
            else Path(args.dir)
    else:
        # Find latest deep_research run
        e2e_dir = Path(__file__).parent.parent / "e2e-output"
        runs = sorted(
            [d for d in e2e_dir.iterdir()
             if d.is_dir() and "deep_research" in d.name],
            reverse=True,
        )
        if not runs:
            print("No deep_research runs found", file=sys.stderr)
            sys.exit(1)
        run_dir = runs[0]

    req_dir = run_dir / "debug" / "requests"
    if not req_dir.exists():
        print(f"No debug/requests/ in {run_dir}", file=sys.stderr)
        sys.exit(1)

    files = sorted(req_dir.glob("*.json"))
    if not files:
        print("No request files found", file=sys.stderr)
        sys.exit(1)

    if args.last:
        analyze_request(files[-1])
    elif args.iter is not None:
        # Find file matching iteration
        matches = [f for f in files if f.name.endswith(f"_{args.iter}.json")]
        if matches:
            for m in matches:
                analyze_request(m)
                print()
        else:
            print(f"No request file for iteration {args.iter}")
            print(f"Available: {[f.name for f in files]}")
    else:
        # Show all
        for f in files:
            analyze_request(f)
            print("=" * 60)
            print()


if __name__ == "__main__":
    main()
