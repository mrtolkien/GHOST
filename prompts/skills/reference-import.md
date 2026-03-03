---
name: reference-import
description:
  Import and query external documentation, code, and API references. Use when the
  OPERATOR asks about a library, framework, SDK, or tool — especially if
  knowledge_search returns no results for it.
---

# Reference Import Skill

Import git repos and web pages as topic-scoped references into the knowledge base.

## CLI Commands

```
ghost reference import --source git --url <url> --topic <name> \
    [--paths dir1,dir2] [--extensions .md,.rs]

ghost reference import --source page --url <url> --topic <name>

ghost reference topics

ghost reference delete --topic <name>
```

## When to Suggest Import

- OPERATOR asks about a library and `knowledge_search` returns no results for it
- OPERATOR wants to study external docs, APIs, or codebases
- OPERATOR asks you to "learn" or "import" a library's documentation
- You find yourself relying on potentially outdated training data for a fast-moving
  library (Dioxus, Leptos, Tauri, etc.)

## Finding the Right Source

Documentation often lives in a separate repo under the same GitHub organization (e.g.
`DioxusLabs/docsite`, not `DioxusLabs/dioxus`). Search before assuming:

```bash
# Search for docs repos in an organization
gh search repos "docs OR docsite OR website" --owner=DioxusLabs --json name,description

# If unclear, check the main repo's README for a docs link
gh api repos/DioxusLabs/dioxus/readme --jq '.content' | base64 -d | head -40

# Or list all repos in the org
gh api orgs/DioxusLabs/repos --jq '.[].name'
```

## Choosing Paths and Extensions (Git)

Once you have the right repo, figure out which subdirectories contain the docs. Use the
GitHub CLI to browse before importing:

```bash
# List top-level directories
gh api repos/DioxusLabs/docsite/contents/ --jq '.[].name'

# Browse a subdirectory
gh api repos/DioxusLabs/docsite/contents/docs-src --jq '.[].name'
```

Pick the narrowest `--paths` that cover the documentation you need. For docs repos, use
`--extensions .md`; for code examples, add `.rs`, `.py`, etc. Omit `--paths` only for
small repos.

## Git Import Details

Uses sparse checkout for large repos — only clones specified paths:

```
ghost reference import --source git \
    --url https://github.com/DioxusLabs/docsite \
    --topic dioxus/docs \
    --paths docs-src/0.7/src/tutorial/ \
    --extensions .md
```

- `--paths`: comma-separated subdirectories (omit for whole repo)
- `--extensions`: file types to include (omit for all text files)
- Idempotent: re-running skips already-imported files
- Creates embeddings for semantic search

## Page Import

```
ghost reference import --source page \
    --url https://docs.rs/sqlx/latest/sqlx/ \
    --topic sqlx/api
```

Fetches page, converts to markdown, stores as reference.

## Post-Import: Enrich the Topic Note

After import, the CLI creates a placeholder index note at `notes/<topic>/index.md` with
a generic body ("Knowledge hub for ..."). You MUST edit this note with a meaningful
description of the library — what it does, what it's used for, key concepts. This is
what makes the topic discoverable via semantic search later.

Example: after importing dioxus docs, edit `notes/dioxus/index.md`:

```markdown
---
title: Dioxus
archetype: topic
tags:
  - dioxus
trust: 5
---

Dioxus is a Rust framework for building cross-platform UIs (web, desktop, mobile).
It uses a React-like component model with RSX syntax, reactive signals for state
management, and a virtual DOM. Key concepts: components, props, hooks, signals,
event handlers, routing.

## Collections

- `dioxus/docs`: tutorial and guide pages from the official docsite
```

## Topic Hierarchy

Topics are pure namespaces: `dioxus`, `dioxus/docs`, `dioxus/source`.

- `dioxus` is a broad topic (parent)
- `dioxus/docs` is a narrower sub-topic
- Searching with `topic="dioxus"` finds results across all sub-topics
- Import metadata (source URL, version, ref count) is stored separately in
  `import_batch` records, not on the topic itself
- Each import writes `_import.toml` alongside the references (e.g.
  `references/dioxus/docs/_import.toml`) with source type, URL, and version

## Post-Import Search

After importing, search scoped to a topic:

```
knowledge_search(query="hooks", topic="dioxus")
knowledge_search(query="connection pool", topic="sqlx", categories=["references"])
```

Or via CLI:

```
ghost knowledge search "hooks" --topic dioxus
```

## Cleanup

Delete a topic and all its references + embeddings:

```
ghost reference delete --topic dioxus/docs
```
