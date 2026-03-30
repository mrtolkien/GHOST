---
title: Reference Import
description:
  Importing external content into the knowledge base — git repositories, web crawls, and
  documents.
---

GHOST can import external content as searchable, embeddable references. Import is a
**two-step flow**: first convert the source to markdown files in a staging directory,
then import those files into the knowledge base under a topic.

Both steps end up storing content the same way: plain text files on disk under
`references/{topic}/`, mirrored in SQLite with FTS5 full-text search and vector
embeddings for semantic search.

## Step 1: Convert

Convert commands fetch and transform sources into markdown files under `.staging/` in
the workspace. The staging directory is cleaned up automatically after a successful
import.

### Git Repositories (preferred)

Best for documentation sets, code examples, and anything in a git repo. Uses sparse
checkout to fetch only the directories and file types you need.

```bash
ghost convert git https://github.com/DioxusLabs/docsite \
    --paths docs-src/0.7/src \
    --extensions .md
```

| Flag             | Purpose                                                      |
| ---------------- | ------------------------------------------------------------ |
| `--paths`        | Comma-separated directories to include (omit for whole repo) |
| `--extensions`   | Comma-separated file extensions to include                   |
| `--ref`          | Pin to a specific branch or tag                              |
| `--output`       | Override staging output directory                            |

### Web Crawl (fallback)

For documentation sites with no git source. BFS-crawls same-host links, converts HTML
to markdown.

```bash
ghost convert crawl https://docs.example.com/ \
    --max-depth 2 \
    --max-pages 30
```

| Flag           | Purpose                                          |
| -------------- | ------------------------------------------------ |
| `--max-depth`  | Maximum BFS depth from the seed URL (default: 3) |
| `--max-pages`  | Maximum pages to crawl (default: 50)             |
| `--output`     | Override staging output directory                |

### PDF and Documents

For PDFs, DOCX, XLSX, PPTX, and images. Conversion is handled by
[Docling](https://github.com/docling-project/docling) — either locally via `uv`
(default) or via a remote
[docling-serve](https://github.com/docling-project/docling-serve) instance.

```bash
ghost convert pdf uploads/paper.pdf
ghost convert pdf uploads/rulebook.pdf --page-range 1-10
```

| Flag            | Purpose                                               |
| --------------- | ----------------------------------------------------- |
| `--no-ocr`      | Disable OCR (faster for born-digital PDFs)            |
| `--page-range`  | Limit pages, e.g. `1-10`                              |
| `--timeout`     | Override conversion timeout in seconds                |
| `--output`      | Override staging output directory                     |

#### Vision fallback for image-heavy pages

Some PDFs (scanned documents, product sheets, image-only pages) produce poor results
through Docling's OCR. GHOST detects this automatically by assessing per-page quality
after conversion — if a page has very little text and is mostly images, it renders the
page to a PNG and sends it to a vision-capable LLM for extraction.

Configure which model to use with `models.vision`:

```toml title="config.toml"
[models]
default = "primary"
vision = "fast"  # Must be a vision-capable model alias
```

If `models.vision` is not set, the default model is used. The fallback only fires for
pages that fail quality checks — it costs nothing for PDFs where Docling succeeds.

#### Conversion backends

By default, GHOST runs Docling locally via a Python script (requires `uv`). You can
configure a remote `docling-serve` instance instead:

```toml title="config.toml"
[docling]
url = "http://127.0.0.1:5001"
```

## Step 2: Import

After converting, import the staging directory (or a single file) as a reference topic:

```bash
ghost reference import .staging/docsite --topic dioxus/docs \
    --source-type git \
    --source-url https://github.com/DioxusLabs/docsite \
    --version-ref abc1234
```

| Flag             | Purpose                                                       |
| ---------------- | ------------------------------------------------------------- |
| `--topic`        | Topic namespace (hierarchical, e.g. `dioxus/docs`)            |
| `--source-type`  | `git`, `crawl`, or `file` — recorded for future updates       |
| `--source-url`   | Original source URL — recorded for future updates             |
| `--version-ref`  | Version identifier (e.g. git commit hash)                     |
| `--git-ref`      | Git branch or tag pinned during convert                       |

The staging directory is removed automatically after a successful import.

:::tip
When GHOST's AI skills run a reference import, they call both commands in sequence and
pass the provenance flags automatically. You only need to supply them manually when
running conversions by hand.
:::

## Staging Directory

The `.staging/` directory in the workspace holds converted markdown files between the
convert and import steps. Each conversion run creates a subdirectory named after the
source (e.g. `.staging/docsite/`).

Files there are readable by GHOST for inspection before committing them to the knowledge
base. After `ghost reference import` succeeds, the staging subdirectory is deleted
automatically.

## Storage Model

Every imported reference is stored in two places:

1. **Disk** — plain text file at `references/{topic}/{filename}`, readable by GHOST via
   `file_read`
2. **Database** — SQLite row with FTS5 indexing for keyword search and vector embeddings
   for semantic search

Import metadata is recorded in:

- `references/{topic}/_import.toml` — source URL, type, paths, extensions, version ref,
  reference count
- `import_batch` DB table — same metadata plus the full import config as JSON, used for
  replay during updates

## Topic Hierarchy

Topics are hierarchical namespaces separated by `/`:

- `dioxus` — parent topic
- `dioxus/docs` — documentation sub-topic
- `dioxus/source` — source code sub-topic

Searching with `topic="dioxus"` finds results across all sub-topics. Each topic level
gets an index note at `notes/{topic}/index.md` — edit this with a meaningful description
so semantic search can discover the topic.

## Updating References

For git and crawl imports, you can re-fetch from the original source to pick up upstream
changes:

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

For git sources, the command short-circuits if the upstream commit hash has not changed.
Use `--ref` to switch to a different branch or tag:

```bash
ghost reference update --topic dioxus/docs --ref v0.6
```

### Orphan Protection

When a file is deleted upstream but a note cites it (via a `cited` edge in the knowledge
graph), the reference is not deleted. Instead it is moved to
`references/{topic}/_orphaned/` and its DB path is updated. A warning is printed so the
OPERATOR can decide what to do.

## Cleanup

Delete a topic and all its references, embeddings, and import metadata:

```bash
ghost reference delete --topic dioxus/docs
```

This removes both the DB records and the workspace files.

## How GHOST Uses These

GHOST's AI skills handle the two-step flow automatically:

- The **reference-import** skill decides between git, crawl, or PDF conversion based on
  the source, then calls `ghost convert <subcommand>` followed by `ghost reference import`
  with the provenance flags populated from the convert output
- Imports run in background mode with the completion watcher triggering a follow-up turn
  when done
- The **knowledge search** tool finds imported references via BM25 and semantic search
- Reflection agents can create `cited` edges linking notes to references
