# /// script
# requires-python = ">=3.11"
# dependencies = ["questionary>=2.0.1"]
# ///
"""Diff agents/ and skills/ between a workspace and the repo source (or another workspace).

Supports plain workspace directories and .tar.zst fixture archives.

Usage:
    uv run scripts/e2e/workspace_diff.py                          # interactive picker
    uv run scripts/e2e/workspace_diff.py <workspace>              # diff vs repo source
    uv run scripts/e2e/workspace_diff.py <workspace> --against <other>

Examples:
    uv run scripts/e2e/workspace_diff.py
    uv run scripts/e2e/workspace_diff.py tests/fixtures/e2e/printer_3d/gpt53/step_01_spawn_agent
    uv run scripts/e2e/workspace_diff.py ~/GHOST --against e2e-output/2026-03-01_step_01
"""

from __future__ import annotations

import argparse
import difflib
import subprocess
import sys
import tempfile
from pathlib import Path

import questionary

try:
    from scripts.e2e._common import REPO_ROOT, list_workspace_dirs
except ModuleNotFoundError:
    from _common import REPO_ROOT, list_workspace_dirs

PROMPTS_DIR = REPO_ROOT / "prompts"

REPO_SOURCE_LABEL = "prompts/ (repo source)"


# --- Extraction ---


def extract_workspace_files(workspace: Path) -> dict[str, str]:
    """Extract agents/ and skills/ content from a workspace.

    Returns a dict mapping logical names to file content:
        "agents/chat-reflection.md" -> content
        "skills/note-writer/skill.md" -> content
    """
    archive = workspace / "workspace.tar.zst"

    if archive.exists():
        return _extract_from_archive(archive)
    return _extract_from_dir(workspace)


def _extract_from_archive(archive: Path) -> dict[str, str]:
    """Extract agents/ and skills/ from a .tar.zst archive."""
    result = subprocess.run(
        ["tar", "--list", "-f", str(archive)],
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        print(f"Error listing archive: {result.stderr}", file=sys.stderr)
        sys.exit(1)

    targets = [
        line.strip()
        for line in result.stdout.splitlines()
        if line.startswith("agents/") or line.startswith("skills/")
    ]
    targets = [t for t in targets if not t.endswith("/") and "." in t.split("/")[-1]]

    if not targets:
        return {}

    with tempfile.TemporaryDirectory() as tmpdir:
        subprocess.run(
            ["tar", "-xf", str(archive), "-C", tmpdir, *targets],
            check=True,
        )
        return _extract_from_dir(Path(tmpdir))


def _extract_from_dir(workspace: Path) -> dict[str, str]:
    """Read agents/ and skills/ from a plain directory."""
    files: dict[str, str] = {}

    agents_dir = workspace / "agents"
    if agents_dir.exists():
        for f in sorted(agents_dir.glob("*.md")):
            files[f"agents/{f.name}"] = f.read_text()

    skills_dir = workspace / "skills"
    if skills_dir.exists():
        for skill_dir in sorted(skills_dir.iterdir()):
            if not skill_dir.is_dir():
                continue
            skill_file = skill_dir / "skill.md"
            if skill_file.exists():
                files[f"skills/{skill_dir.name}/skill.md"] = skill_file.read_text()

    return files


def load_repo_source() -> dict[str, str]:
    """Load current agent/skill source from prompts/ using the same key scheme."""
    files: dict[str, str] = {}

    agents_dir = PROMPTS_DIR / "agents"
    if agents_dir.exists():
        for f in sorted(agents_dir.glob("*.md")):
            files[f"agents/{f.name}"] = f.read_text()

    skills_dir = PROMPTS_DIR / "skills"
    if skills_dir.exists():
        for f in sorted(skills_dir.glob("*.md")):
            files[f"skills/{f.stem}/skill.md"] = f.read_text()

    return files


# --- Diff ---


def diff_files(
    left: dict[str, str],
    right: dict[str, str],
    left_label: str,
    right_label: str,
) -> bool:
    """Print unified diffs between two file sets. Returns True if any differ."""
    all_keys = sorted(set(left) | set(right))
    has_diff = False

    for key in all_keys:
        l_content = left.get(key)
        r_content = right.get(key)

        if l_content is None:
            print(f"\n+++ {key}  (only in {right_label})")
            has_diff = True
            continue
        if r_content is None:
            print(f"\n--- {key}  (only in {left_label})")
            has_diff = True
            continue
        if l_content == r_content:
            continue

        has_diff = True
        diff = difflib.unified_diff(
            l_content.splitlines(keepends=True),
            r_content.splitlines(keepends=True),
            fromfile=f"{left_label}/{key}",
            tofile=f"{right_label}/{key}",
            n=3,
        )
        sys.stdout.writelines(diff)

    return has_diff


# --- Interactive pickers ---


def pick_workspace(prompt: str, exclude: Path | None = None) -> tuple[str, Path] | None:
    """Interactive picker for workspace directories. Returns (label, path) or None for repo source."""
    choices = list_workspace_dirs()
    if exclude:
        choices = [(l, p) for l, p in choices if p != exclude]

    labels = [c[0] for c in choices]
    labels.insert(0, REPO_SOURCE_LABEL)

    selected = questionary.select(prompt, choices=labels).ask()
    if not selected:
        raise SystemExit("No selection")

    if selected == REPO_SOURCE_LABEL:
        return None

    idx = labels.index(selected) - 1  # offset for the repo source entry
    return choices[idx]


# --- CLI ---


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Diff workspace agents/skills against repo source or another workspace",
    )
    parser.add_argument(
        "workspace",
        nargs="?",
        type=Path,
        help="Workspace dir (or fixture dir with workspace.tar.zst). "
        "Omit for interactive picker.",
    )
    parser.add_argument(
        "--against",
        type=Path,
        default=None,
        help="Compare against another workspace instead of repo source",
    )
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> None:
    args = parse_args(argv)

    # --- Resolve left side ---
    if args.workspace:
        if not args.workspace.exists():
            print(f"Not found: {args.workspace}", file=sys.stderr)
            sys.exit(1)
        left_label = str(args.workspace)
        left = extract_workspace_files(args.workspace)
    else:
        picked = pick_workspace("Select LEFT workspace to diff")
        if picked is None:
            print("Cannot use repo source as left side.", file=sys.stderr)
            sys.exit(1)
        left_label, left_path = picked
        left = extract_workspace_files(left_path)

    # --- Resolve right side ---
    if args.against:
        if not args.against.exists():
            print(f"Not found: {args.against}", file=sys.stderr)
            sys.exit(1)
        right_label = str(args.against)
        right = extract_workspace_files(args.against)
    elif args.workspace:
        # CLI mode with explicit workspace: default to repo source
        right_label = REPO_SOURCE_LABEL
        right = load_repo_source()
    else:
        # Interactive: pick right side too
        picked = pick_workspace("Select RIGHT side (or repo source)")
        if picked is None:
            right_label = REPO_SOURCE_LABEL
            right = load_repo_source()
        else:
            right_label, right_path = picked
            right = extract_workspace_files(right_path)

    if not left:
        print(f"No agents/skills found in {left_label}", file=sys.stderr)
        sys.exit(1)

    print(f"Comparing: {left_label}")
    print(f"  against: {right_label}")
    print(f"  files:   {len(left)} left, {len(right)} right")
    print()

    has_diff = diff_files(left, right, left_label, right_label)

    if not has_diff:
        print("No differences found.")


if __name__ == "__main__":
    main()
