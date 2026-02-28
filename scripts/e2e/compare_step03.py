# /// script
# dependencies = []
# ///
"""Compare all step_03 reflection runs: standard vs fork."""

import json
from pathlib import Path

RUNS = [
    ("standard", "e2e-output/2026-02-28T03-27-59_printer_3d_step_03_reflect_agent"),
    ("fork",     "e2e-output/2026-02-28T03-29-50_printer_3d_step_03_reflect_agent"),
    ("standard", "e2e-output/2026-02-28T03-47-55_printer_3d_step_03_reflect_agent"),
    ("fork",     "e2e-output/2026-02-28T03-49-47_printer_3d_step_03_reflect_agent"),
    ("standard", "e2e-output/2026-02-28T03-52-12_printer_3d_step_03_reflect_agent"),
    ("fork",     "e2e-output/2026-02-28T03-53-09_printer_3d_step_03_reflect_agent"),
]

# Research session ends before these timestamps; reflection files start after.
REFLECTION_CUTOFF = "20260228T032600"


def parse_usage_from_response(response) -> dict | None:
    # Response may be SSE text or already-parsed dict
    if isinstance(response, dict):
        return response.get("usage")
    for line in response.split("\n"):
        if not line.startswith("data: "):
            continue
        try:
            event = json.loads(line[6:])
        except json.JSONDecodeError:
            continue
        if event.get("type") in ("response.completed", "response.incomplete"):
            return event.get("response", {}).get("usage", {})
    return None


def extract_reflection_usage(run_dir: Path, approach: str):
    debug_dir = run_dir / "debug" / "requests"
    files = sorted(debug_dir.glob("*.json"))

    # Identify reflection session: for standard it's a different session ID,
    # for fork it's the same session but files after the cutoff.
    # Find all session IDs
    sessions: dict[str, list] = {}
    for f in files:
        parts = f.stem.split("_")
        if len(parts) >= 3:
            sid = parts[1]
            sessions.setdefault(sid, []).append(f)

    if approach == "standard":
        # Reflection is the session that starts AFTER the cutoff
        reflection_files = []
        for sid, sfiles in sessions.items():
            if all(f.name >= REFLECTION_CUTOFF for f in sfiles):
                reflection_files = sfiles
                break
        if not reflection_files:
            # Fallback: session with fewest files that isn't the research one
            research_sid = max(sessions, key=lambda s: len(sessions[s]))
            for sid, sfiles in sessions.items():
                if sid != research_sid:
                    reflection_files = sfiles
                    break
    else:
        # Fork: same session, but files after cutoff
        research_sid = max(sessions, key=lambda s: len(sessions[s]))
        reflection_files = [
            f for f in sessions[research_sid] if f.name >= REFLECTION_CUTOFF
        ]

    results = []
    for f in sorted(reflection_files):
        data = json.loads(f.read_text())
        usage = parse_usage_from_response(data.get("response", ""))
        if not usage:
            continue
        cached = usage.get("input_tokens_details", {}).get("cached_tokens", 0)
        inp = usage.get("input_tokens", 0)
        results.append({
            "input": inp,
            "output": usage.get("output_tokens", 0),
            "cache_read": cached,
            "cold": cached == 0,
        })
    return results


def analyze_notes(run_dir: Path):
    notes_dir = run_dir / "notes"
    notes = [f for f in notes_dir.rglob("*.md") if f.name != "index.md"]
    total_words = 0
    total_wikilinks = 0
    archetypes: dict[str, int] = {}
    for n in notes:
        text = n.read_text()
        total_words += len(text.split())
        total_wikilinks += text.count("[[")
        # Parse archetype from frontmatter
        for line in text.split("\n"):
            if line.startswith("archetype:"):
                arch = line.split(":", 1)[1].strip()
                archetypes[arch] = archetypes.get(arch, 0) + 1
                break
    return {
        "count": len(notes),
        "words": total_words,
        "wikilinks": total_wikilinks,
        "avg_words": total_words / len(notes) if notes else 0,
        "avg_links": total_wikilinks / len(notes) if notes else 0,
        "archetypes": archetypes,
    }


def analyze_references(run_dir: Path):
    refs_dir = run_dir / "references"
    if not refs_dir.exists():
        return 0
    return len(list(refs_dir.rglob("*.md")))


def main():
    rows = []
    for approach, path_str in RUNS:
        run_dir = Path(path_str)
        ts = run_dir.name[:23]

        usage = extract_reflection_usage(run_dir, approach)
        notes = analyze_notes(run_dir)
        refs = analyze_references(run_dir)

        total_input = sum(u["input"] for u in usage)
        total_output = sum(u["output"] for u in usage)
        total_cached = sum(u["cache_read"] for u in usage)
        non_cached = total_input - total_cached
        pct = (total_cached / total_input * 100) if total_input > 0 else 0
        hits = sum(1 for u in usage if u["cache_read"] > 0)

        # For fork: the first cold iteration(s) would be warm in real flow
        # because the research session cache is still hot. Simulate by
        # treating cold fork iterations as if they had ~98% cache (the rate
        # observed on all subsequent warm iterations).
        if approach == "fork":
            adj_cached = total_cached
            for u in usage:
                if u["cold"]:
                    adj_cached += int(u["input"] * 0.98)
            adj_non_cached = total_input - adj_cached
        else:
            # Standard always has a truly cold iter 0 (new session/prompt)
            adj_cached = total_cached
            adj_non_cached = non_cached

        rows.append({
            "approach": approach,
            "ts": ts,
            "iterations": len(usage),
            "input": total_input,
            "output": total_output,
            "cached": total_cached,
            "non_cached": non_cached,
            "adj_non_cached": adj_non_cached,
            "cache_pct": pct,
            "cache_hits": hits,
            "notes": notes["count"],
            "words": notes["words"],
            "wikilinks": notes["wikilinks"],
            "avg_words": notes["avg_words"],
            "avg_links": notes["avg_links"],
            "archetypes": notes["archetypes"],
            "refs": refs,
        })

    # Print per-run table
    print()
    print("=" * 100)
    print("  STEP_03 REFLECTION: 6-Run Comparison (3 standard, 3 fork)")
    print("=" * 100)

    hdr = (
        f"  {'Approach':<10} {'Iters':>5} {'Input':>10} {'Cached':>10} "
        f"{'Cache%':>7} {'Hits':>5} {'NonCach':>10} {'AdjNC':>10} {'Output':>8} "
        f"{'Notes':>5} {'Words':>6} {'Links':>5} {'W/N':>5} {'L/N':>5} {'Refs':>4}"
    )
    print(hdr)
    print("  " + "-" * (len(hdr) - 2))

    for r in rows:
        print(
            f"  {r['approach']:<10} {r['iterations']:>5} "
            f"{r['input']:>10,} {r['cached']:>10,} "
            f"{r['cache_pct']:>6.1f}% {r['cache_hits']:>3}/{r['iterations']:<1} "
            f"{r['non_cached']:>10,} {r['adj_non_cached']:>10,} {r['output']:>8,} "
            f"{r['notes']:>5} {r['words']:>6,} {r['wikilinks']:>5} "
            f"{r['avg_words']:>5.0f} {r['avg_links']:>5.1f} {r['refs']:>4}"
        )

    # Averages by approach
    print()
    print("=" * 100)
    print("  AVERAGES")
    print("=" * 100)
    for approach in ("standard", "fork"):
        group = [r for r in rows if r["approach"] == approach]
        n = len(group)
        print(f"\n  {approach.upper()} (n={n}):")
        avg = lambda key: sum(r[key] for r in group) / n
        print(f"    Iterations:   {avg('iterations'):>8.1f}")
        print(f"    Input tokens: {avg('input'):>10,.0f}")
        print(f"    Cached:       {avg('cached'):>10,.0f} ({avg('cache_pct'):.1f}%)")
        print(f"    Non-cached:   {avg('non_cached'):>10,.0f}")
        print(f"    Adj non-cach: {avg('adj_non_cached'):>10,.0f}")
        print(f"    Output:       {avg('output'):>8,.0f}")
        print(f"    Notes:        {avg('notes'):>8.1f}")
        print(f"    Words:        {avg('words'):>8,.0f}")
        print(f"    Wikilinks:    {avg('wikilinks'):>8.1f}")
        print(f"    Avg words/n:  {avg('avg_words'):>8.0f}")
        print(f"    Avg links/n:  {avg('avg_links'):>8.1f}")
        print(f"    References:   {avg('refs'):>8.1f}")

    # Fork vs standard ratios
    std = [r for r in rows if r["approach"] == "standard"]
    frk = [r for r in rows if r["approach"] == "fork"]
    sa = lambda key: sum(r[key] for r in std) / len(std)
    fa = lambda key: sum(r[key] for r in frk) / len(frk)

    print()
    print("=" * 100)
    print("  FORK vs STANDARD RATIOS")
    print("=" * 100)
    for label, key in [
        ("Non-cached input", "non_cached"),
        ("Adj non-cached", "adj_non_cached"),
        ("Total input", "input"),
        ("Output tokens", "output"),
        ("Notes", "notes"),
        ("Words", "words"),
        ("Wikilinks", "wikilinks"),
        ("References", "refs"),
    ]:
        s, f = sa(key), fa(key)
        ratio = f / s if s > 0 else float("inf")
        print(f"  {label:<20} standard={s:>10,.0f}  fork={f:>10,.0f}  ratio={ratio:.2f}x")

    print()


if __name__ == "__main__":
    main()
