# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""
Analyze step_03 (reflect agent) output to diagnose curation failures.

Checks: notes → source URLs → web cache URL matching → why curate_references
might move 0 files.
"""

import os
import re
import sys
from pathlib import Path

def slug_from_url(url: str) -> str:
    """Mirror the Rust slug_from_url logic."""
    stripped = url
    for prefix in ["https://", "http://"]:
        if stripped.startswith(prefix):
            stripped = stripped[len(prefix):]
    if stripped.startswith("www."):
        stripped = stripped[4:]

    # sanitize_slug
    slug = ""
    for c in stripped:
        slug += c.lower() if c.isalnum() else "-"

    # Collapse consecutive dashes
    result = ""
    prev_dash = False
    for c in slug:
        if c == "-":
            if not prev_dash and result:
                result += "-"
            prev_dash = True
        else:
            result += c
            prev_dash = False

    result = result.rstrip("-")
    if len(result) > 60:
        result = result[:60].rstrip("-")
    return result


def extract_frontmatter(content: str) -> dict:
    """Extract YAML-ish frontmatter."""
    if not content.startswith("---"):
        return {}
    end = content.find("---", 3)
    if end == -1:
        return {}
    fm = content[3:end].strip()
    result = {}
    for line in fm.splitlines():
        if ":" in line and not line.startswith("-"):
            key, val = line.split(":", 1)
            result[key.strip()] = val.strip()
        elif line.startswith("- ") and "sources" in result:
            if isinstance(result["sources"], str):
                result["sources"] = []
            result["sources"].append(line[2:].strip())
    # Handle multi-line sources
    return result


def extract_note_sources(content: str) -> list[str]:
    """Extract source URLs from note frontmatter."""
    urls = []
    in_sources = False
    lines = content.splitlines()
    in_front = False
    for line in lines:
        if line.strip() == "---":
            if not in_front:
                in_front = True
                continue
            else:
                break
        if in_front:
            if line.startswith("sources:"):
                in_sources = True
                continue
            if in_sources:
                if line.startswith("- "):
                    urls.append(line[2:].strip())
                else:
                    in_sources = False
    return urls


def extract_body_urls(content: str) -> list[str]:
    """Extract URLs from body text (after frontmatter)."""
    # Skip frontmatter
    if content.startswith("---"):
        end = content.find("---", 3)
        if end != -1:
            content = content[end + 3:]
    return re.findall(r"https?://[^\s\]\)>,]+", content)


def main():
    base = sys.argv[1] if len(sys.argv) > 1 else None
    if not base:
        # Find the latest step_03 output
        output_dir = Path("e2e-output")
        candidates = sorted(
            [d for d in output_dir.iterdir() if "step_03" in d.name],
            key=lambda p: p.name,
            reverse=True,
        )
        if not candidates:
            print("No step_03 output found")
            sys.exit(1)
        base = str(candidates[0])

    workspace = Path(base)
    print(f"Workspace: {workspace}")

    # 1. Collect web cache files and their URLs
    cache_dir = workspace / ".web-cache"
    cache_files = {}
    if cache_dir.exists():
        for f in sorted(cache_dir.iterdir()):
            if f.suffix == ".md":
                content = f.read_text()
                fm = extract_frontmatter(content)
                url = fm.get("url", "")
                is_search = "query" in fm
                slug = slug_from_url(url) if url else ""
                cache_files[f.name] = {
                    "url": url,
                    "slug": slug,
                    "is_search": is_search,
                    "size": f.stat().st_size,
                }
    print(f"\n# Web cache files: {len(cache_files)}")
    for name, info in cache_files.items():
        flag = " [SEARCH]" if info["is_search"] else ""
        print(f"  {name}: slug={info['slug'][:50]}...{flag} ({info['size']}B)")

    # 2. Collect notes and their source URLs
    notes_dir = workspace / "notes"
    note_urls = []
    notes_found = []
    if notes_dir.exists():
        for f in sorted(notes_dir.rglob("*.md")):
            content = f.read_text()
            sources = extract_note_sources(content)
            body_urls = extract_body_urls(content)
            rel = f.relative_to(notes_dir)
            notes_found.append(str(rel))
            for url in sources:
                note_urls.append(("source", str(rel), url))
            for url in body_urls:
                note_urls.append(("body", str(rel), url))

    print(f"\n# Notes: {len(notes_found)}")
    for n in notes_found:
        print(f"  {n}")

    print(f"\n# URLs found in notes: {len(note_urls)}")
    for kind, note, url in note_urls:
        slug = slug_from_url(url)
        print(f"  [{kind}] {note}: {url}")
        print(f"    slug: {slug}")

    # 3. Check matching: for each cache file, does any note URL match?
    print(f"\n# Curation analysis:")
    matched = 0
    unmatched = 0
    for name, info in cache_files.items():
        if not info["url"]:
            print(f"  {name}: NO URL → would be deleted")
            unmatched += 1
            continue

        file_slug = info["slug"]
        # Check if cited in agent findings (we'd need the findings for this)
        # Check if URL appears in notes
        matching = False
        match_details = []
        for kind, note, url in note_urls:
            note_slug = slug_from_url(url)
            if (
                note_slug == file_slug
                or file_slug.startswith(note_slug)
                or note_slug.startswith(file_slug)
            ):
                matching = True
                match_details.append(f"{kind} in {note}")

        if matching:
            matched += 1
            print(f"  {name}: MATCH → would be moved to references/")
            for d in match_details:
                print(f"    matched by: {d}")
        else:
            unmatched += 1
            print(f"  {name}: NO MATCH → would be deleted")
            print(f"    file slug: {file_slug}")

    print(f"\n# Summary: {matched} matched, {unmatched} unmatched")
    if matched == 0:
        print("WARNING: 0 files would be moved to references!")
        print("This means curate_references will produce an empty references/ dir")


if __name__ == "__main__":
    main()
