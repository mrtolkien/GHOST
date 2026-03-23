# /// script
# requires-python = ">=3.11"
# dependencies = ["rich"]
# ///
"""
Vendor upstream skill repos into vendor/<name>/.

Usage:
    uv run scripts/sync-vendor.py superpowers          # fetch + show diff
    uv run scripts/sync-vendor.py superpowers --apply   # update vendor dir
    uv run scripts/sync-vendor.py anthropic-skills      # fetch + show diff
"""

import argparse
import difflib
import shutil
import subprocess
import tempfile
from pathlib import Path

from rich.console import Console
from rich.syntax import Syntax

ROOT = Path(__file__).resolve().parent.parent

VENDORS = {
    "superpowers": "https://github.com/obra/superpowers.git",
    "anthropic-skills": "https://github.com/anthropics/skills.git",
}

console = Console()


def clone_repo(repo_url: str, tmp: Path) -> Path:
    subprocess.run(
        ["git", "clone", "--depth=1", repo_url, str(tmp / "repo")],
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


def load_vendored(vendor_dir: Path) -> dict[str, dict[str, str]]:
    result: dict[str, dict[str, str]] = {}
    if not vendor_dir.exists():
        return result
    for skill_dir in sorted(vendor_dir.iterdir()):
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
    vendor_name: str,
    old_skills: dict[str, dict[str, str]],
    new_skills: dict[str, dict[str, str]],
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
                fromfile=f"vendor/{vendor_name}/{name}/{filename}",
                tofile=f"upstream/{name}/{filename}",
            )
            console.print(f"\n[bold]{name}/{filename}[/bold]:")
            console.print(Syntax("".join(diff), "diff"))
    return changed


def apply(vendor_dir: Path, new_skills: dict[str, dict[str, str]]) -> None:
    if vendor_dir.exists():
        shutil.rmtree(vendor_dir)
    vendor_dir.mkdir(parents=True)
    total_files = 0
    for name, files in sorted(new_skills.items()):
        skill_dir = vendor_dir / name
        skill_dir.mkdir()
        for filename, content in sorted(files.items()):
            dest = skill_dir / filename
            dest.parent.mkdir(parents=True, exist_ok=True)
            dest.write_text(content)
            total_files += 1
    console.print(
        f"\n[green]Vendored {len(new_skills)} skills ({total_files} files) to {vendor_dir}[/green]"
    )
    console.print(
        "[yellow]Review diffs and port changes to assets/skills/ manually.[/yellow]"
    )


def main() -> None:
    parser = argparse.ArgumentParser(description="Sync upstream skill repos")
    parser.add_argument(
        "vendor",
        choices=sorted(VENDORS),
        help="Which vendor to sync",
    )
    parser.add_argument("--apply", action="store_true", help="Update vendor dir")
    args = parser.parse_args()

    repo_url = VENDORS[args.vendor]
    vendor_dir = ROOT / "vendor" / args.vendor

    with tempfile.TemporaryDirectory() as tmp:
        console.print(f"[dim]Cloning {repo_url}...[/dim]")
        repo = clone_repo(repo_url, Path(tmp))
        new_skills = collect_skills(repo)

    console.print(f"Found {len(new_skills)} upstream skills")

    old_skills = load_vendored(vendor_dir)
    changed = show_diff(args.vendor, old_skills, new_skills)

    if not changed:
        console.print("[green]No changes from upstream.[/green]")
        return

    if args.apply:
        apply(vendor_dir, new_skills)
    else:
        console.print("\n[yellow]Run with --apply to update vendor dir.[/yellow]")


if __name__ == "__main__":
    main()
