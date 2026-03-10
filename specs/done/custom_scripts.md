# Custom Scripts — Spec

## Overview

The GHOST should write clean, reusable scripts when answering questions that require
code. Instead of throwaway shell one-liners or `python3 -c`, the GHOST produces proper
scripts in `$WORKSPACE/scripts/{topic}/` that are indexed as a first-class knowledge
category and discoverable via semantic search.

This feature has three parts:

1. **Scripting skill** — opinionated guidelines for how the GHOST writes scripts
2. **Script knowledge category** — scripts as a searchable knowledge type (like
   notes/references/diary)
3. **E2e tests** — verify the GHOST follows the guidelines end-to-end

## Design Philosophy

Scripts are **executable artifacts**, not reference material. The GHOST doesn't just
_read_ them for information — it _runs_ them to produce results. This is fundamentally
different from references (source material you consult) or notes (interpretations you
reason from). Scripts deserve their own knowledge category because the GHOST's intent
when searching for them is different: "can I _do_ this?" vs "what do I _know_ about
this?"

## 1. Scripting Skill

A workspace skill at `$WORKSPACE/skills/scripting/` that the GHOST reads before writing
any code. The skill matches on questions that require running code, data processing, API
calls, or anything beyond a single coreutils command.

### Core Rules

- **Location**: `$WORKSPACE/scripts/{topic}/{name}.py` — topic-based organization
- **PEP 723 inline metadata**: Every script declares dependencies via `# /// script`
  block. See https://docs.astral.sh/uv/guides/scripts/
- **Module docstring**: First thing after metadata. Describes what the script does and
  when to use it. This is what gets embedded and makes semantic search work.
- **typer** for scripts with arguments (automatic `--help`, type-checked args). Plain
  `if __name__ == "__main__"` for no-arg scripts.
- **Python by default**. See "Non-Python Scripts" below for when other languages are a
  better fit.
- **One task per script**. Keep them small and focused — most scripts should be a single
  chunk for embeddings (~2000 chars).
- **Run with** `uv run scripts/{topic}/{name}.py [args]`
- **Import library docs** via `reference import` (git/crawl/page) if unfamiliar with a
  library's API. Don't guess at APIs.

### Example Script

```python
# /// script
# requires-python = ">=3.12"
# dependencies = ["python-whois", "typer"]
# ///
"""Check domain expiry dates.

Looks up WHOIS records for a list of domains and warns if any are
expiring within a threshold (default 30 days). Domains: tolki.dev,
tachikoma-ai.com.
"""

import typer
# ... implementation
```

### When to Script vs Not

- Single coreutils command (ls, grep, wc, df) → just run it
- Needs a library, logic, parsing, or API calls → write a script
- Would benefit from reuse → write a script

### Non-Python Scripts

Python is the default because uv handles dependency management seamlessly and the
ecosystem covers most tasks. Use a different language when:

- **Python lacks the right library** or the best tool is native to another ecosystem
  (e.g., Go CLI tools, Rust performance-critical processing)
- **The task is a thin shell wrapper** around existing CLI tools — use Bash

Guidelines for non-Python scripts:

- **Bash**: `scripts/{topic}/{name}.sh`. Add a shebang (`#!/usr/bin/env bash`), set
  `set -euo pipefail`. Use for orchestrating existing CLI tools. No complex logic — if
  you need conditionals or loops over data, use Python instead.
- **Other compiled languages** (Go, Rust, etc.): Add required toolchains to the
  workspace nix shell (`shell/`) so the script can be built and run. Document the build
  step in the script's header comment.
- Always include a descriptive header comment (equivalent to the Python docstring) for
  embedding quality.

## 2. Script Knowledge Category

Scripts become a first-class knowledge source alongside notes, references, and diary.

### Database

New `script` table:

```sql
CREATE TABLE script (
    id          TEXT PRIMARY KEY,  -- ULID
    path        TEXT NOT NULL UNIQUE,  -- relative to workspace: scripts/finance/spending.py
    content     TEXT NOT NULL,
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);
```

FTS5 index:

```sql
CREATE VIRTUAL TABLE script_fts USING fts5(
    path,
    content,
    content=script,
    content_rowid=rowid,
    tokenize='porter unicode61'
);
```

With the usual sync triggers (INSERT, UPDATE, DELETE) to keep FTS5 in sync.

### Reconciliation

The filesystem watcher/reconciler (which already handles notes, references, diary)
extends to watch `$WORKSPACE/scripts/`. On file create/update/delete:

- Upsert/remove the `script` table record
- Queue embedding (AST-aware tree-sitter chunking already supports Python and Bash)
- Content hash tracking skips re-embedding unchanged scripts

### Search Integration

`knowledge_search` gains `"scripts"` as a category:

```json
{
  "query": "check domain expiry",
  "categories": ["scripts"]
}
```

- BM25 via `script_fts` (path + content)
- Vector search via existing `embedding` table (source_table = "script")
- Hybrid merge same as other categories (0.4 BM25 + 0.6 vector)

### Embedding Quality

Qwen3-Embedding-8B scores 80.68 on MTEB Code benchmark (state-of-the-art). Natural
language queries like "check if my domains are expiring" reliably match against Python
scripts containing WHOIS logic and domain names. The module docstring is the primary
semantic anchor — the convention of writing a clear, descriptive docstring ensures good
retrieval.

### How Chunking Works for Scripts

The code chunker (`src/embeddings/chunker.rs`) uses tree-sitter with a 2000-char target:

1. Parse the file into an AST (Python, Bash, etc.)
2. If the root module node is <= 2000 chars → **single chunk** (whole file). This is the
   common case for well-focused scripts.
3. If > 2000 chars → recurse into top-level AST children (imports, docstring, function
   definitions, class definitions)
4. `greedy_merge` recombines consecutive small nodes back up to 2000 chars

So for a typical script: the module docstring, imports (which reveal dependencies), and
early code all land in the first chunk — ideal for semantic search. The "one task per
script" guideline keeps most scripts in single-chunk territory.

## 3. User Stories

### US1 — Monthly spending from bank CSV

**Prompt**: OPERATOR drops a CSV and asks "How much did I spend on food this month?"

**Expected behavior**: GHOST reads the scripting skill → writes
`scripts/finance/spending_by_category.py` with PEP 723 metadata (csv or pandas dep),
typer for category filtering → runs it on the CSV → returns formatted breakdown.

**Test fixture**: Mock CSV with categorized transactions (groceries, restaurants, rent,
utilities). Assert script exists, has PEP 723 block, has docstring, uses typer (has
arguments). Run the script and verify numeric output.

### US2 — Domain expiry check

**Prompt**: "Check if tolki.dev and tachikoma-ai.com are expiring soon"

**Expected behavior**: GHOST writes `scripts/domains/check_expiry.py` with python-whois
dep → runs it → returns expiry dates with warnings for anything < 30 days.

**Test**: Assert script exists, has PEP 723 block with python-whois dependency, has
docstring. GHOST runs the script as part of its response (hits real WHOIS servers).

### US3 — Weather forecast

**Prompt**: "What's the weather going to be like this week near Tokyo station, Tokyo?"

**Expected behavior**: GHOST writes `scripts/weather/forecast.py` with httpx dep,
Open-Meteo API (no key needed) → runs it → returns formatted weekly forecast.

**Test**: Assert script exists, has PEP 723 block with HTTP client dependency, has
docstring. GHOST runs the script as part of its response (hits real API).

## 4. E2e Test Structure

Reorganize existing `tests/daemon_e2e.rs` into a folder:

```
tests/
  daemon.rs                # feature-gated entry point
  daemon/
    helpers.rs             # shared daemon test setup
    ark_nova.rs            # existing reference import test (renamed)
    scripting.rs           # 3 scripting scenarios (US1, US2, US3)
```

Feature flag: `live-tests` (same as existing daemon test).

### Shared Assertions for Scripting Tests

Every scripting test asserts:

1. Script file exists under `$WORKSPACE/scripts/{topic}/`
2. File contains `# /// script` PEP 723 metadata block
3. File contains a module docstring (triple-quoted string near top)
4. GHOST ran the script (check for `run_shell_command` tool use with `uv run`)

US1 additionally asserts:

- Script uses typer (has CLI arguments)
- Script output contains numeric spending data when run against the fixture CSV
