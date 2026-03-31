---
name: reference-import
description:
  Import and update external content in the knowledge base — git repos, web crawls, and
  documents (PDF, DOCX). Use when the OPERATOR wants persistent, searchable reference
  material from a git repository, website, or document, or when existing references may
  be stale and need refreshing.
---

# Reference Import Skill

Import git repos, web crawls, and documents as topic-scoped references into the
knowledge base. All sources go through a two-step flow: **convert** (produce staging dir
with markdown) then **import** (index into the knowledge base).

## Decision Flow

Follow this order — stop as soon as you have an answer:

1. **Search first**: `knowledge_search(query="<topic>", categories=["references"])`. If
   results exist, use them to answer. Done.
2. **Git import** (preferred for whole doc sets): find the docs repo via `gh`, convert
   with `ghost convert git`, then import. Use `background: true` for the convert step.
3. **Crawl import** (fallback): only if no git source exists (e.g. docs-only site).
4. **PDF/Document import**: download first (curl), then `ghost convert pdf`, then
   import.
5. **After starting the background convert**: tell the OPERATOR it's importing, include
   any other pending responses (project offers, plans, etc.), then **end your turn**. A
   follow-up turn is triggered automatically when the convert finishes — you'll see the
   `[shell-command completed]` system message. Read the staging dir, pick a topic, then
   run the import. Note: reference records appear in the DB almost immediately; only the
   embeddings trail behind.

## Two-Step Flow

### Step 1: Convert

Convert produces a staging directory with markdown files and prints the staging path to
stdout:

```
ghost convert git <url> [--paths dir1,dir2] [--extensions .md,.rs] [--git-ref <ref>]
ghost convert crawl <url> [--max-depth 3] [--max-pages 50]
ghost convert pdf <path> [--no-ocr] [--page-range "1-10"] [--timeout 900]
```

The staging directory defaults to `<workspace>/.staging/<slug>/`. The command prints the
staging path and provenance details (source URL, git ref, etc.) to stdout — capture
these for the import step.

### Step 2: Inspect

After the convert completes, read a few files from the staging dir to understand the
content, then pick an appropriate `--topic`.

### Step 3: Import

```
ghost reference import <staging-dir> --topic <topic> \
    [--source-type git|crawl|file] \
    [--source-url <url>] \
    [--version-ref <commit-hash>] \
    [--git-ref <branch-or-tag>]
```

The import indexes the staging directory into the knowledge base and writes files to
`references/{topic}/`. Pass the provenance flags from the convert output so the import
record is fully traceable.

## Git Import (Preferred)

### Finding the docs repo

Documentation often lives in a separate repo (e.g. `DioxusLabs/docsite`, not
`DioxusLabs/dioxus`). One search is enough:

```bash
gh search repos "docs OR docsite OR website" --owner=<Org> --json name,description
```

### Choosing paths + extensions

Browse the repo to pick the narrowest `--paths`:

```bash
gh api repos/<owner>/<repo>/contents/ --jq '.[].name'
```

For docs repos use `--extensions .md`; for code examples add `.rs`, `.py`, etc. Omit
`--paths` only for small repos.

### Running the convert (background)

Git converts clone the repo and embed every file, which is slow on CPU. **Always use
background mode** for the convert step:

```json
{
  "command": "ghost convert git https://github.com/DioxusLabs/docsite --paths docs-src/0.7/src --extensions .md",
  "background": true
}
```

Tell the OPERATOR: _"I'm converting the Dioxus docs in the background — I'll import them
once conversion finishes."_ Finish any other pending responses, then **end your turn**.
The completion watcher triggers a follow-up turn automatically.

### After convert completes

When you see `[shell-command completed]`, the stdout contains the staging path. Read a
few files to pick a good topic name, then run the import:

```json
{
  "command": "ghost reference import /path/to/.staging/docsite --topic dioxus/docs --source-type git --source-url https://github.com/DioxusLabs/docsite --git-ref main"
}
```

## Crawl Import (Fallback)

Use only when no git source exists:

```json
{
  "command": "ghost convert crawl https://docs.example.com/ --max-depth 2 --max-pages 30",
  "background": true
}
```

After convert completes, import the staging dir:

```json
{
  "command": "ghost reference import /path/to/.staging/docs-example-com --topic example/docs --source-type crawl --source-url https://docs.example.com/"
}
```

## PDF / Document Import

### Why download first?

Many sites serve consent pages, CAPTCHAs, or redirects instead of the actual file.
Downloading first lets you verify the file is real (check size, file type) before
converting. It also keeps the original in `uploads/` for re-import if needed.

### Workflow

1. **Download** (foreground — verify the file is real before converting):

```json
{
  "command": "curl -L -o uploads/rulebook.pdf 'https://example.com/rulebook.pdf'",
  "background": false
}
```

Check file size and type — some sites return HTML error pages with HTTP 200.

2. **Convert** (background — OCR can take minutes):

```json
{
  "command": "ghost convert pdf uploads/rulebook.pdf",
  "background": true
}
```

3. **Import** after convert completes:

```json
{
  "command": "ghost reference import /path/to/.staging/rulebook --topic boardgames/arknova --source-type file"
}
```

### PDF convert options (use ONLY when explicitly requested)

These are optimization overrides. **Use defaults unless the OPERATOR asks otherwise.**

| Flag                  | Default  | When to use                               |
| --------------------- | -------- | ----------------------------------------- |
| `--no-ocr`            | OCR on   | OPERATOR says PDF is digital, wants speed |
| `--page-range "1-10"` | full doc | OPERATOR wants specific pages only        |
| `--timeout 900`       | config   | OPERATOR needs more time for huge docs    |

Do NOT guess at these options. Do NOT add `--no-ocr` to "speed things up". The OPERATOR
will tell you if they want non-default behavior.

### Uploaded files

When the OPERATOR uploads a file, it lands in `uploads/` in the workspace. Convert it
directly — no curl needed:

```json
{
  "command": "ghost convert pdf uploads/<filename>",
  "background": true
}
```

After import, the original file is preserved in `references/<topic>/_originals/`. Clean
up the uploaded file:

```json
{
  "command": "rm uploads/<filename>"
}
```

## Files on Disk

All imported references are written to disk under `references/{topic}/` in the workspace
AND stored in the DB for search. This means you can `file_read` on paths returned by
`knowledge_search` to get full reference content — no need to re-fetch from the web.

- **Git imports**: files mirror the repo structure (`references/{topic}/{rel_path}`)
- **Crawl imports**: filenames are URL slugs (`references/{topic}/{slug}.md`)
- **PDF imports**: one markdown file per source document
- `knowledge_search` results for references include a `path:` field you can `file_read`

## Staging Directory

The staging directory (`.staging/` in workspace) is auto-cleaned after a successful
import. If you need to re-import with different options, re-run the convert step.

## Post-Import: Enrich the Topic Note

After import, a placeholder note exists at `notes/<topic>/index.md`. Edit it with a
meaningful description — what the library/document covers, key concepts. This makes the
topic discoverable via semantic search.

## Topic Hierarchy

Topics are namespaces: `dioxus`, `dioxus/docs`, `dioxus/source`.

- Searching with `topic="dioxus"` finds results across all sub-topics
- Each import writes `_import.toml` alongside the references

## Post-Import Search

```
knowledge_search(query="hooks", topic="dioxus", categories=["references"])
```

## Cleanup

```
ghost reference delete --topic dioxus/docs
```

## Updating References

When the OPERATOR asks to refresh or update existing reference material, or when you
notice imported docs may be stale (e.g. a library released a new version):

```
ghost reference update --topic <name> [--ref <tag-or-branch>]
```

This re-fetches from the original source and applies changes:

- New files are added
- Changed files are updated (content + embeddings)
- Files deleted upstream are removed — unless cited by notes, in which case they're
  moved to `_orphaned/` with a warning

For git sources, the command short-circuits if the upstream commit hash hasn't changed.
Use `--ref` to switch to a different branch or tag.

Examples:

```json
{ "command": "ghost reference update --topic dioxus/docs", "background": true }
{ "command": "ghost reference update --topic dioxus/docs --ref v0.6", "background": true }
```

## CLI Reference

```
# Convert
ghost convert git <url> [--paths <paths>] [--extensions <exts>] [--git-ref <ref>] [--output <dir>]
ghost convert crawl <url> [--max-depth 3] [--max-pages 50] [--output <dir>]
ghost convert pdf <path> [--no-ocr] [--page-range "1-10"] [--timeout <secs>] [--output <dir>]

# Import
ghost reference import <staging-dir> --topic <name> \
    [--source-type git|crawl|file] \
    [--source-url <url>] \
    [--version-ref <commit-hash>] \
    [--git-ref <branch-or-tag>]

# Manage
ghost reference update --topic <name> [--ref <tag-or-branch>]
ghost reference delete --topic <name>
ghost topics list
```
