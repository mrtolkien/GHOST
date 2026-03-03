---
name: reference-import
description:
  Import external documentation and code as references scoped by topic. Use when the
  OPERATOR asks to learn about a library, framework, or external resource.
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

- OPERATOR asks about a library not in the knowledge base
- OPERATOR wants to study external docs, APIs, or codebases
- A `knowledge_search` returns no results for a known library

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
