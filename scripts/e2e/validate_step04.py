# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Validate step_04 output quality: final response, diary, reflection tools."""

import json
import sys
from pathlib import Path

OUTPUT_DIR = Path("e2e-output")

# Find latest step_04 output
step04_dirs = sorted(OUTPUT_DIR.glob("*step_04*"), reverse=True)
if not step04_dirs:
    print("No step_04 output found")
    sys.exit(1)

d = step04_dirs[0]
print(f"Inspecting: {d.name}\n")

# --- State & final response ---
state_file = Path(
    "tests/fixtures/e2e/printer_3d/gpt53/"
    "step_04_finalize_chat_and_reflect/state.json"
)
if state_file.exists():
    state = json.loads(state_file.read_text())
    final = state.get("assertion_markers", {}).get("final_response", "")
    preview = state.get("final_response_preview", "")
    print("=== FINAL RESPONSE ===")
    print(f"Length: {len(final)} chars")
    print(f"Preview: {preview[:200]}...")
    print()

    # Quality checks
    keywords = [
        "Bambu",
        "P2S",
        "QIDI",
        "Plus4",
        "Prusa",
        "CORE One",
        "price",
        "enclosed",
    ]
    found = [k for k in keywords if k.lower() in final.lower()]
    missing = [k for k in keywords if k.lower() not in final.lower()]
    print(f"Keywords found ({len(found)}/{len(keywords)}): {found}")
    if missing:
        print(f"Keywords MISSING: {missing}")
    print()

    # Check structure
    has_table = "|" in final and "---" in final
    has_headings = "##" in final
    has_sources = "http" in final
    source_count = final.count("http")
    print(f"Has table: {has_table}")
    print(f"Has headings: {has_headings}")
    print(f"Has sources: {has_sources} ({source_count} URLs)")
    print()

# --- Diary ---
diary_dir = d / "diary"
diary_files = list(diary_dir.glob("*.md")) if diary_dir.exists() else []
print("=== DIARY ===")
if diary_files:
    for df in diary_files:
        content = df.read_text().strip()
        print(f"File: {df.name} ({len(content)} chars)")
        print(content)
        print()

        # Quality: diary should mention the research topic
        diary_keywords = ["3D printer", "printer", "research", "Bambu"]
        diary_found = [
            k for k in diary_keywords if k.lower() in content.lower()
        ]
        print(
            f"Diary keywords ({len(diary_found)}/{len(diary_keywords)}):"
            f" {diary_found}"
        )
else:
    print("NO DIARY FILES FOUND - this is a problem!")
print()

# --- Debug requests: identify sessions and iterations ---
debug_dir = d / "debug" / "requests"
if debug_dir.exists():
    files = sorted(debug_dir.glob("*.json"))
    sessions: dict[str, list[str]] = {}
    for f in files:
        parts = f.stem.split("_")
        if len(parts) >= 2:
            sess = parts[1]
            sessions.setdefault(sess, []).append(f.name)

    print("=== API REQUESTS ===")
    for sess, reqs in sessions.items():
        print(f"Session {sess}: {len(reqs)} requests")

        # Check last request size for context pressure
        last_req = debug_dir / reqs[-1]
        try:
            data = json.loads(last_req.read_text())
            msgs = data.get("messages", [])
            total_chars = sum(
                len(str(b))
                for m in msgs
                for b in m.get("content", [])
            )
            print(
                f"  Last request: {len(msgs)} messages,"
                f" ~{total_chars:,} chars"
            )

            # Check roles distribution
            roles: dict[str, int] = {}
            for m in msgs:
                r = m.get("role", "?")
                roles[r] = roles.get(r, 0) + 1
            print(f"  Roles: {roles}")
        except Exception as e:
            print(f"  Error reading {reqs[-1]}: {e}")
    print()

# --- Notes and references ---
notes_dir = d / "notes"
refs_dir = d / "references"
notes = list(notes_dir.glob("*.md")) if notes_dir.exists() else []
refs = list(refs_dir.glob("*.md")) if refs_dir.exists() else []
print(f"=== WORKSPACE ===")
print(f"Notes: {len(notes)}")
for n in notes:
    print(f"  {n.name} ({n.stat().st_size}B)")
print(f"References: {len(refs)}")
for r in refs:
    print(f"  {r.name} ({r.stat().st_size}B)")
