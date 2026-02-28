# /// script
# requires-python = ">=3.11"
# dependencies = ["questionary>=2.0.1"]
# ///
"""
Evaluate step_03 (reflect agent) output: extract metrics, print report,
optionally append to JSONL evaluation log.

Usage:
    uv run scripts/e2e/evaluate_step03.py                # interactive picker
    uv run scripts/e2e/evaluate_step03.py <fixture-dir>  # specific dir
    uv run scripts/e2e/evaluate_step03.py --no-append     # print only
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

import questionary

try:
    from scripts.e2e._common import FIXTURES_ROOT, REPO_ROOT, list_step_dirs, step_label
except ModuleNotFoundError:
    from _common import FIXTURES_ROOT, REPO_ROOT, list_step_dirs, step_label

EVALUATIONS_DIR = REPO_ROOT / "tests" / "evaluations"
EVAL_FILE = EVALUATIONS_DIR / "printer_3d_03.jsonl"


def parse_toml_frontmatter(content: str) -> dict[str, str]:
    """Extract TOML frontmatter key-value pairs (simple flat parsing)."""
    if not content.startswith("+++"):
        # Try YAML-style ---
        if not content.startswith("---"):
            return {}
        end = content.find("---", 3)
        if end == -1:
            return {}
        fm_text = content[3:end].strip()
    else:
        end = content.find("+++", 3)
        if end == -1:
            return {}
        fm_text = content[3:end].strip()

    result: dict[str, str] = {}
    current_key = ""
    for line in fm_text.splitlines():
        stripped = line.strip()
        if ":" in stripped and not stripped.startswith("-"):
            key, val = stripped.split(":", 1)
            current_key = key.strip()
            val = val.strip().strip('"').strip("'")
            result[current_key] = val
        elif stripped.startswith("- ") and current_key:
            existing = result.get(current_key, "")
            if existing:
                result[current_key] = existing + "," + stripped[2:].strip()
            else:
                result[current_key] = stripped[2:].strip()
    return result


def count_wiki_links(text: str) -> int:
    """Count [[...]] wiki links in text."""
    return len(re.findall(r"\[\[[^\]]+\]\]", text))


def get_body(content: str) -> str:
    """Get body text after frontmatter."""
    for delim in ("---", "+++"):
        if content.startswith(delim):
            end = content.find(delim, len(delim))
            if end != -1:
                return content[end + len(delim):].strip()
    return content


def get_commit_hash() -> str:
    """Get current git HEAD short hash."""
    try:
        result = subprocess.run(
            ["git", "rev-parse", "--short", "HEAD"],
            capture_output=True, text=True, cwd=str(REPO_ROOT),
        )
        return result.stdout.strip()
    except Exception:
        return "unknown"


def extract_metrics(workspace: Path) -> dict:
    """Extract all evaluation metrics from a step_03 workspace."""
    metrics: dict = {}

    # --- metrics.json ---
    metrics_path = workspace / "metrics.json"
    if not metrics_path.exists():
        # Try parent fixture dir (metrics.json is at step level)
        metrics_path = workspace.parent / "metrics.json" if workspace.parent else metrics_path
    if metrics_path.exists():
        mdata = json.loads(metrics_path.read_text())
        reflection = mdata.get("reflection", {})
        metrics["wall_clock_secs"] = reflection.get("wall_clock_secs", 0)
        metrics["input_tokens"] = reflection.get("input_tokens", 0)
        metrics["output_tokens"] = reflection.get("output_tokens", 0)
        metrics["cache_read_tokens"] = reflection.get("cache_read_tokens", 0)
        metrics["iterations"] = reflection.get("iterations", 0)
        metrics["tool_counts"] = reflection.get("tool_counts", {})
    else:
        metrics["wall_clock_secs"] = 0
        metrics["input_tokens"] = 0
        metrics["output_tokens"] = 0
        metrics["cache_read_tokens"] = 0
        metrics["iterations"] = 0
        metrics["tool_counts"] = {}

    # --- Notes ---
    notes_dir = workspace / "notes"
    note_files = []
    if notes_dir.exists():
        note_files = [
            f for f in notes_dir.rglob("*.md")
            if f.name != "index.md"
        ]

    metrics["notes_created"] = len(note_files)

    decision_count = 0
    source_quality_count = 0
    total_wiki_links = 0
    word_counts: list[int] = []

    for nf in note_files:
        content = nf.read_text()
        fm = parse_toml_frontmatter(content)
        body = get_body(content)

        # Archetype
        archetype = fm.get("archetype", "").lower()
        if archetype == "decision":
            decision_count += 1

        # Source quality heuristic: title contains " — " or archetype
        title = fm.get("title", "")
        if " — " in title or "source" in archetype:
            source_quality_count += 1

        # Wiki links
        total_wiki_links += count_wiki_links(body)

        # Word count
        words = len(body.split())
        word_counts.append(words)

    metrics["decision_notes"] = decision_count
    metrics["source_quality_notes"] = source_quality_count
    metrics["total_wiki_links"] = total_wiki_links
    metrics["avg_note_words"] = (
        round(sum(word_counts) / len(word_counts))
        if word_counts else 0
    )

    # --- References ---
    refs_dir = workspace / "references"
    ref_files = []
    if refs_dir.exists():
        ref_files = list(refs_dir.rglob("*"))
        ref_files = [f for f in ref_files if f.is_file()]
    metrics["references_curated"] = len(ref_files)

    # --- Web cache remaining ---
    cache_dir = workspace / ".web-cache"
    cache_files = []
    if cache_dir.exists():
        cache_files = [f for f in cache_dir.iterdir() if f.is_file()]
    metrics["web_cache_remaining"] = len(cache_files)

    return metrics


def check_test_passed(workspace: Path) -> bool:
    """Heuristic: test passed if notes and references both exist."""
    notes_dir = workspace / "notes"
    refs_dir = workspace / "references"
    has_notes = notes_dir.exists() and any(notes_dir.rglob("*.md"))
    has_refs = refs_dir.exists() and any(refs_dir.rglob("*"))
    return has_notes and has_refs


def print_report(metrics: dict, workspace: Path) -> None:
    """Print a human-readable evaluation report."""
    print(f"\n{'='*60}")
    print(f"  Step 03 Evaluation: {workspace.name}")
    print(f"{'='*60}\n")

    print(f"  Wall clock:          {metrics['wall_clock_secs']:.1f}s")
    print(f"  Input tokens:        {metrics['input_tokens']:,}")
    print(f"  Output tokens:       {metrics['output_tokens']:,}")
    print(f"  Cache read tokens:   {metrics['cache_read_tokens']:,}")
    print(f"  Iterations:          {metrics['iterations']}")
    print()
    print(f"  Notes created:       {metrics['notes_created']}")
    print(f"  Decision notes:      {metrics['decision_notes']}")
    print(f"  Source quality notes: {metrics['source_quality_notes']}")
    print(f"  Total wiki links:    {metrics['total_wiki_links']}")
    print(f"  Avg note words:      {metrics['avg_note_words']}")
    print(f"  References curated:  {metrics['references_curated']}")
    print(f"  Web cache remaining: {metrics['web_cache_remaining']}")

    if metrics.get("tool_counts"):
        print(f"\n  Tool counts:")
        for name, count in sorted(metrics["tool_counts"].items()):
            print(f"    {name}: {count}")

    print(f"\n{'='*60}")


def build_record(
    metrics: dict,
    approach: str,
    model: str,
    notes: str,
    test_passed: bool,
) -> dict:
    """Build a JSONL evaluation record."""
    return {
        "timestamp": datetime.now(timezone.utc).isoformat(),
        "approach": approach,
        "model": model,
        "commit_hash": get_commit_hash(),
        "test_passed": test_passed,
        "wall_clock_secs": metrics["wall_clock_secs"],
        "input_tokens": metrics["input_tokens"],
        "output_tokens": metrics["output_tokens"],
        "cache_read_tokens": metrics["cache_read_tokens"],
        "iterations": metrics["iterations"],
        "notes_created": metrics["notes_created"],
        "decision_notes": metrics["decision_notes"],
        "source_quality_notes": metrics["source_quality_notes"],
        "total_wiki_links": metrics["total_wiki_links"],
        "references_curated": metrics["references_curated"],
        "web_cache_remaining": metrics["web_cache_remaining"],
        "avg_note_words": metrics["avg_note_words"],
        "tool_counts": metrics["tool_counts"],
        "notes": notes,
    }


def pick_workspace() -> Path:
    """Interactive picker for step_03 fixtures or e2e-output dirs."""
    choices: list[tuple[str, Path]] = []

    # Fixture step dirs
    for sd in list_step_dirs():
        if "step_03" in sd.step:
            choices.append((f"[fixture] {step_label(sd)}", sd.path))

    # e2e-output dirs
    output_root = REPO_ROOT / "e2e-output"
    if output_root.exists():
        for d in sorted(output_root.iterdir(), reverse=True):
            if d.is_dir() and "step_03" in d.name:
                choices.append((f"[output] {d.name}", d))

    if not choices:
        raise SystemExit("No step_03 directories found")

    labels = [c[0] for c in choices]
    selected = questionary.select(
        "Select step_03 workspace to evaluate",
        choices=labels,
    ).ask()
    if not selected:
        raise SystemExit("No selection")

    idx = labels.index(selected)
    return choices[idx][1]


def resolve_workspace(path: Path) -> Path:
    """Given a fixture step dir or e2e-output dir, find the workspace root.

    For fixture dirs, the workspace IS the extracted archive content (state
    is at step level, workspace archive unpacks to step dir level).
    For e2e-output dirs, the workspace IS the directory itself.
    """
    # If it has notes/ or .web-cache/, it's already a workspace
    if (path / "notes").exists() or (path / ".web-cache").exists():
        return path
    # If it has workspace.tar.zst, it's a fixture dir — workspace
    # content is in the archive, but we can't easily extract it.
    # The metrics.json is at step level though.
    return path


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Evaluate step_03 reflection output"
    )
    parser.add_argument("dir", nargs="?", type=Path, help="Workspace or fixture dir")
    parser.add_argument(
        "--no-append", action="store_true",
        help="Print report only, don't append to JSONL"
    )
    parser.add_argument("--approach", default="standard", help="Approach label")
    parser.add_argument("--model", help="Model alias (auto-detected from state.json)")
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> None:
    args = parse_args(argv)

    if args.dir:
        workspace_dir = args.dir
        if not workspace_dir.exists():
            raise SystemExit(f"Not found: {workspace_dir}")
    else:
        workspace_dir = pick_workspace()

    workspace = resolve_workspace(workspace_dir)

    # Try to detect model from state.json
    model = args.model
    if not model:
        state_path = workspace_dir / "state.json"
        if state_path.exists():
            state = json.loads(state_path.read_text())
            model = state.get("model_alias", "unknown")
        else:
            model = "unknown"

    metrics = extract_metrics(workspace)
    test_passed = check_test_passed(workspace)
    print_report(metrics, workspace_dir)
    print(f"  Test passed:         {test_passed}")
    print(f"  Model:               {model}")
    print(f"  Commit:              {get_commit_hash()}")

    if args.no_append:
        return

    # Prompt for notes
    notes_text = questionary.text(
        "Notes for this evaluation (free text, optional):",
        default="",
    ).ask()
    if notes_text is None:
        notes_text = ""

    record = build_record(metrics, args.approach, model, notes_text, test_passed)

    EVALUATIONS_DIR.mkdir(parents=True, exist_ok=True)
    with open(EVAL_FILE, "a") as f:
        f.write(json.dumps(record, ensure_ascii=False) + "\n")

    print(f"\nAppended to {EVAL_FILE.relative_to(REPO_ROOT)}")


if __name__ == "__main__":
    main()
