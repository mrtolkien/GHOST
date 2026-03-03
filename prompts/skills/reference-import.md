---
name: reference-import
description:
  Import and query external documentation, code, and API references. Use when the
  OPERATOR asks about a library, framework, SDK, or tool — especially if
  knowledge_search returns no results for it.
---

# Reference Import Skill

Import git repos and web pages as topic-scoped references into the knowledge base.

## Decision Flow

Follow this order — stop as soon as you have an answer:

1. **Search first**: `knowledge_search(query="<topic>", categories=["references"])`. If
   results exist, use them to answer. Done.
2. **Git import** (preferred): find the docs repo via `gh`, import with
   `background: true`, tell the OPERATOR it's importing.
3. **Crawl import** (fallback): only if no git source exists (e.g. docs-only site).
4. **After import completes**: the result arrives as a `[shell-command completed]`
   system message on the OPERATOR's next turn (there is no auto-trigger — the message
   sits in the DB until the OPERATOR sends a follow-up). Search the imported refs, edit
   the topic note, and answer. Note: reference records appear in the DB almost
   immediately; only the embeddings trail behind. You can search whatever's embedded so
   far.

## CLI Commands

```
ghost reference import --source git --url <url> --topic <name> \
    [--paths dir1,dir2] [--extensions .md,.rs]

ghost reference import --source page --url <url> --topic <name>

ghost reference import --source crawl --url <url> --topic <name> \
    [--max-depth 3] [--max-pages 50]

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
  "command": "ghost reference import --source git --url https://github.com/DioxusLabs/docsite --topic dioxus/docs --paths docs-src/0.7/src --extensions .md",
  "background": true
}
```

Tell the OPERATOR: _"I'm importing the Dioxus docs in the background — I'll search them
once the import finishes."_

When the OPERATOR follows up, you'll see the `[shell-command completed]` system message
(if the import finished). Search the imported references and answer the OPERATOR's
question. If the import is still running, search whatever's been embedded so far and
supplement with web search if needed.

## Crawl Import (Fallback)

Use only when no git source exists:

```json
{
  "command": "ghost reference import --source crawl --url https://docs.example.com/ --topic example/docs --max-depth 2 --max-pages 30",
  "background": true
}
```

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
