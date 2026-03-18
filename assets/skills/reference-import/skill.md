---
name: reference-import
description:
  Import and update external content in the knowledge base — git repos and web crawls.
  Use when knowledge_search has no results for a topic, when the OPERATOR wants
  persistent, searchable reference material from a git repository or website, or when
  existing references may be stale and need refreshing. For PDF/DOCX/file imports, use
  the document-import skill instead.
---

# Reference Import Skill

Import git repos and web crawls as topic-scoped references into the knowledge base.

## Decision Flow

Follow this order — stop as soon as you have an answer:

1. **Search first**: `knowledge_search(query="<topic>", categories=["references"])`. If
   results exist, use them to answer. Done.
2. **Document import** (PDF, DOCX, uploaded file): use the `document-import` skill
   instead. This skill covers git repos and web crawls only.
3. **Git import** (preferred for whole doc sets): find the docs repo via `gh`, import
   with `background: true`, tell the OPERATOR it's importing.
4. **Crawl import** (fallback): only if no git source exists (e.g. docs-only site).
5. **After starting the background import**: tell the OPERATOR it's importing, include
   any other pending offers or responses (e.g. project creation), then **end your
   turn**. A follow-up turn is triggered automatically when the import completes —
   you'll see the `[shell-command completed]` system message. Search the imported refs
   and answer. Note: reference records appear in the DB almost immediately; only the
   embeddings trail behind. You can search whatever's embedded so far.

## CLI Commands

```
ghost reference import git --url <url> --topic <name> \
    [--paths dir1,dir2] [--extensions .md,.rs] [--ref <tag-or-branch>]

ghost reference import crawl --url <url> --topic <name> \
    [--max-depth 3] [--max-pages 50]

ghost reference update --topic <name> [--ref <tag-or-branch>]

ghost topics list

ghost reference delete --topic <name>
```

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

### Running the import (background)

Git imports embed every file, which is slow on CPU. **Always use background mode**:

```json
{
  "command": "ghost reference import git --url https://github.com/DioxusLabs/docsite --topic dioxus/docs --paths docs-src/0.7/src --extensions .md",
  "background": true
}
```

Tell the OPERATOR: _"I'm importing the Dioxus docs in the background — I'll search them
once the import finishes."_ Finish any other pending responses (project offers, plans,
etc.), then **end your turn** — the completion watcher will automatically trigger a
follow-up turn when the import finishes. You'll see the `[shell-command completed]`
system message and can search the imported references.

## Crawl Import (Fallback)

Use only when no git source exists:

```json
{
  "command": "ghost reference import crawl --url https://docs.example.com/ --topic example/docs --max-depth 2 --max-pages 30",
  "background": true
}
```

## Files on Disk

All imported references are written to disk under `references/{topic}/` in the workspace
AND stored in the DB for search. This means you can `file_read` on paths returned by
`knowledge_search` to get full reference content — no need to re-fetch from the web.

- **Git imports**: files mirror the repo structure (`references/{topic}/{rel_path}`)
- **Crawl imports**: filenames are URL slugs (`references/{topic}/{slug}.md`)
- `knowledge_search` results for references include a `path:` field you can `file_read`

## Post-Import: Enrich the Topic Note

After import, a placeholder note exists at `notes/<topic>/index.md`. Edit it with a
meaningful description — what the library does, key concepts. This makes the topic
discoverable via semantic search.

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
