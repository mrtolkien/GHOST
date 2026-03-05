# /// script
# requires-python = ">=3.11"
# dependencies = ["rich"]
# ///
"""
Vendor superpowers skills from obra/superpowers into vendor/superpowers/.

Usage:
    uv run scripts/sync-superpowers.py          # fetch + show diff
    uv run scripts/sync-superpowers.py --apply  # update vendor dir
"""

import argparse
import difflib
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

from rich.console import Console
from rich.syntax import Syntax

ROOT = Path(__file__).resolve().parent.parent
VENDOR_DIR = ROOT / "vendor" / "superpowers"
REPO_URL = "https://github.com/obra/superpowers.git"

console = Console()


def clone_repo(tmp: Path) -> Path:
    subprocess.run(
        ["git", "clone", "--depth=1", REPO_URL, str(tmp / "repo")],
        check=True,
        capture_output=True,
    )
    return tmp / "repo"


def collect_skills(repo: Path) -> dict[str, dict[str, str]]:
    skills_dir = repo / "skills"
    result: dict[str, dict[str, str]] = {}
    if not skills_dir.exists():
        return result
    for skill_dir in sorted(skills_dir.iterdir()):
        if not skill_dir.is_dir():
            continue
        files: dict[str, str] = {}
        for f in sorted(skill_dir.rglob("*")):
            if not f.is_file():
                continue
            try:
                files[str(f.relative_to(skill_dir))] = f.read_text()
            except UnicodeDecodeError:
                continue
        if files:
            result[skill_dir.name] = files
    return result


def show_diff(
    old_skills: dict[str, dict[str, str]], new_skills: dict[str, dict[str, str]]
) -> bool:
    changed = False
    all_names = sorted(set(old_skills) | set(new_skills))
    for name in all_names:
        old_files = old_skills.get(name, {})
        new_files = new_skills.get(name, {})
        all_filenames = sorted(set(old_files) | set(new_files))
        for filename in all_filenames:
            old = old_files.get(filename, "")
            new = new_files.get(filename, "")
            if old == new:
                continue
            changed = True
            diff = difflib.unified_diff(
                old.splitlines(keepends=True),
                new.splitlines(keepends=True),
                fromfile=f"vendor/{name}/{filename}",
                tofile=f"upstream/{name}/{filename}",
            )
            console.print(f"\n[bold]{name}/{filename}[/bold]:")
            console.print(Syntax("".join(diff), "diff"))
    return changed


def load_vendored() -> dict[str, dict[str, str]]:
    result: dict[str, dict[str, str]] = {}
    if not VENDOR_DIR.exists():
        return result
    for skill_dir in sorted(VENDOR_DIR.iterdir()):
        if not skill_dir.is_dir():
            continue
        files: dict[str, str] = {}
        for f in sorted(skill_dir.rglob("*")):
            if not f.is_file():
                continue
            try:
                files[str(f.relative_to(skill_dir))] = f.read_text()
            except UnicodeDecodeError:
                continue
        if files:
            result[skill_dir.name] = files
    return result


def apply(new_skills: dict[str, dict[str, str]]) -> None:
    if VENDOR_DIR.exists():
        shutil.rmtree(VENDOR_DIR)
    VENDOR_DIR.mkdir(parents=True)
    total_files = 0
    for name, files in sorted(new_skills.items()):
        skill_dir = VENDOR_DIR / name
        skill_dir.mkdir()
        for filename, content in sorted(files.items()):
            dest = skill_dir / filename
            dest.parent.mkdir(parents=True, exist_ok=True)
            dest.write_text(content)
            total_files += 1
    console.print(
        f"\n[green]Vendored {len(new_skills)} skills ({total_files} files) to {VENDOR_DIR}[/green]"
    )
    console.print("[yellow]Review diffs and port changes to prompts/skills/ manually.[/yellow]")


def main() -> None:
    parser = argparse.ArgumentParser(description="Sync superpowers skills")
    parser.add_argument("--apply", action="store_true", help="Update vendor dir")
    args = parser.parse_args()

    with tempfile.TemporaryDirectory() as tmp:
        console.print("[dim]Cloning obra/superpowers...[/dim]")
        repo = clone_repo(Path(tmp))
        new_skills = collect_skills(repo)

    console.print(f"Found {len(new_skills)} upstream skills")

    old_skills = load_vendored()
    changed = show_diff(old_skills, new_skills)

    if not changed:
        console.print("[green]No changes from upstream.[/green]")
        return

    if args.apply:
        apply(new_skills)
    else:
        console.print("\n[yellow]Run with --apply to update vendor dir.[/yellow]")


if __name__ == "__main__":
    main()
