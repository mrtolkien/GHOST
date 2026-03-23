---
title: Reference Import
description:
  Importing external content into the knowledge base — git repositories,
  web crawls, and documents.
---

GHOST can import external content as searchable, embeddable references.
There are two main import paths: **repository/web import** for git repos
and websites, and **document import** for PDFs, DOCX, and other files.

Both paths store content the same way: plain text files on disk under
`references/{topic}/`, mirrored in SQLite with FTS5 full-text search and
vector embeddings for semantic search.

## Import Paths

### Git Repositories (preferred)

Best for documentation sets, code examples, and anything in a git repo.
Uses sparse checkout to fetch only the directories and file types you
need.

```bash
ghost reference import git \
    --url https://github.com/DioxusLabs/docsite \
    --topic dioxus/docs \
    --paths docs-src/0.7/src \
    --extensions .md
```

| Flag | Purpose |
| --- | --- |
| `--url` | Repository URL |
| `--topic` | Topic namespace (hierarchical, e.g. `dioxus/docs`) |
| `--paths` | Comma-separated directories to include (omit for whole repo) |
| `--extensions` | Comma-separated file extensions to include |
| `--ref` | Pin to a specific branch or tag |

### Web Crawl (fallback)

For documentation sites with no git source. BFS-crawls same-host links,
converts HTML to markdown.

```bash
ghost reference import crawl \
    --url https://docs.example.com/ \
    --topic example/docs \
    --max-depth 2 \
    --max-pages 30
```

### Document Import

For PDFs, DOCX, XLSX, PPTX, and images. Requires a running
[docling-serve](https://github.com/docling-project/docling-serve)
instance for conversion.

```bash
# Download first, then import
curl -L -o uploads/paper.pdf https://example.com/paper.pdf
ghost document import file --path uploads/paper.pdf --topic papers/ml

# From a local/uploaded file
ghost document import file --path uploads/rulebook.pdf --topic boardgames/arknova
```

Originals are preserved in `references/{topic}/_originals/`.

## Storage Model

Every imported reference is stored in two places:

1. **Disk** — plain text file at `references/{topic}/{filename}`, readable
   by GHOST via `file_read`
2. **Database** — SQLite row with FTS5 indexing for keyword search and
   vector embeddings for semantic search

Import metadata is recorded in:
- `references/{topic}/_import.toml` — source URL, type, paths, extensions,
  version ref, reference count
- `import_batch` DB table — same metadata plus the full import config as
  JSON, used for replay during updates

## Topic Hierarchy

Topics are hierarchical namespaces separated by `/`:

- `dioxus` — parent topic
- `dioxus/docs` — documentation sub-topic
- `dioxus/source` — source code sub-topic

Searching with `topic="dioxus"` finds results across all sub-topics.
Each topic level gets an index note at `notes/{topic}/index.md` — edit
this with a meaningful description so semantic search can discover the
topic.

## Updating References

For git and crawl imports, you can re-fetch from the original source to
pick up upstream changes:

```bash
ghost reference update --topic dioxus/docs
```

The update command:

1. Reads the saved import config from `_import.toml` (or DB fallback)
2. Re-fetches the full manifest from the source
3. Compares each file by content hash
4. **New files** — added to disk and DB
5. **Changed files** — overwritten on disk, updated in DB
6. **Deleted files** — removed, unless cited by notes (see below)
7. Updates `_import.toml` and import batch metadata

For git sources, the command short-circuits if the upstream commit hash
has not changed. Use `--ref` to switch to a different branch or tag:

```bash
ghost reference update --topic dioxus/docs --ref v0.6
```

### Orphan Protection

When a file is deleted upstream but a note cites it (via a `cited` edge
in the knowledge graph), the reference is not deleted. Instead it is
moved to `references/{topic}/_orphaned/` and its DB path is updated. A
warning is printed so the OPERATOR can decide what to do.

## Cleanup

Delete a topic and all its references, embeddings, and import metadata:

```bash
ghost reference delete --topic dioxus/docs
```

This removes both the DB records and the workspace files.

## How GHOST Uses These

GHOST's AI skills handle the decision flow automatically:

- The **reference-import** skill decides between git, crawl, or document
  import based on the source
- Imports run in background mode with the completion watcher triggering a
  follow-up turn when done
- The **knowledge search** tool finds imported references via BM25 and
  semantic search
- Reflection agents can create `cited` edges linking notes to references
