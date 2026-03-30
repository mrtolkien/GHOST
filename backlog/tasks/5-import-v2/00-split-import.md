# Split Reference Import Into Convert + Import

The current reference import system (`ghost reference import git|crawl`, `ghost document
import file`) bundles source fetching/conversion with reference storage in one command.
This causes:

- **Hard to debug** — one big process, usually running in background
- **Topic must be known upfront** — impossible for documents the GHOST hasn't read yet
- **Partial failure leaves artifacts** — DB records without files, or files without DB

Split into two distinct commands: `ghost convert` (source -> markdown) and
`ghost reference import` (markdown -> references).

---

## Design

### `ghost convert` — source to markdown, no DB

Three subcommands, each producing a staging directory of markdown files:

```
ghost convert pdf <path> [--no-ocr] [--page-range 1-10] [--timeout 300] [--output <dir>]
ghost convert git <url> [--paths docs/ src/] [--extensions .md .rst] [--ref main]
ghost convert crawl <url> [--max-depth 3] [--max-pages 50]
```

**Output:**

- Default staging path: `{workspace}/staging/{auto-slug}/` — slug derived from source
  (e.g., `dioxus-docs`, `example-com`, `quarterly-report`)
- `--output` override available for all subcommands
- Staging directory contains only markdown files, preserving relative structure from source
  - PDF: single `{stem}.md`
  - Git: filtered file tree as-is
  - Crawl: `{url-slug}.md` per page, flat
- Prints to stdout: staging directory path + provenance metadata (source_type, source_url,
  version_ref, git_ref as applicable)
- Zero DB interaction, zero workspace config required beyond knowing the staging path
- Exit 0 on success, non-zero with error to stderr on failure

**Error handling:**

- Fail clearly, don't push through partial results silently
- Git clone fails / URL unreachable / docling fails -> error to stderr, non-zero exit, no
  staging dir created
- Partial crawl (some pages fail) -> still fails, reports which pages couldn't be fetched

### `ghost reference import` — markdown to references + DB

```
ghost reference import <path> --topic <topic> \
  [--source-type git|crawl|file] \
  [--source-url <url>] \
  [--version-ref <hash>] \
  [--git-ref <branch>]
```

**`<path>`**: a file or a directory.

- File: imports a single markdown file to `references/{topic}/{filename}`
- Directory: imports all markdown files recursively, preserving internal directory
  structure under `references/{topic}/`

**`--topic`** (required): hierarchical topic name (e.g., `dioxus/docs`). The GHOST
decides this after reading the converted content.

**Provenance flags** (optional): `--source-type`, `--source-url`, `--version-ref`,
`--git-ref`. The GHOST passes these through from the convert step's stdout output. Stored
in `_import.toml` and DB import batch, enabling `reference update` to re-fetch later.

**What it does:**

1. Validate path exists (file or directory)
2. `ensure_topic_hierarchy()` — create topic + parents in DB
3. For each markdown file:
   - Write to `references/{topic}/{relative_path}` (preserving directory structure)
   - Compute `content_hash()`
   - `create_reference()` in DB
   - Skip if reference at that path already exists (idempotent)
4. `upsert_import_batch()` with provenance and ref count
5. `write_import_toml()` to `references/{topic}/_import.toml`
6. `ensure_index_notes()` for the topic
7. Delete staging directory on success (leave it on failure for retry/inspection)
8. Print summary (created/skipped counts)

**Error handling:**

- Path doesn't exist -> clear error
- No markdown files found in directory -> error
- DB failure midway -> files already on disk stay, GHOST can retry
- Clear errors propagated, never swallowed

### `reference update` — unchanged command, new internals

```
ghost reference update --topic <topic> [--ref <override>]
```

Reads `_import.toml` for provenance (same as today). Internally calls `convert::git` or
`convert::crawl` to produce a fresh staging dir, diffs against existing
`references/{topic}/` by content hash, creates/updates/deletes/orphans as before. Cleans
up temporary staging dir when done.

### `reference delete` — no changes

Already works correctly. No changes needed.

### `cli/document.rs` — removed

Replaced by `ghost convert pdf`. The `document` command group goes away.

## Module Structure

```
src/
  convert/
    mod.rs          # barrel
    pdf.rs          # PDF -> markdown via docling
    git.rs          # git clone -> markdown tree (from fetch_git_manifest)
    crawl.rs        # BFS crawl -> markdown files (from fetch_crawl_manifest)
    staging.rs      # staging dir creation, slug generation, stdout format

  reference_import/
    mod.rs          # barrel
    import.rs       # generic "take path, write to references/ + DB"
                    # import_from_path() is the single entry point — used by CLI,
                    # reference update, AND web cache curation
    topic.rs        # ensure_topic_hierarchy, write_import_toml (stays)
    update.rs       # reference update (calls convert/ for re-fetch, then diffs)
    types.rs        # ImportResult, UpdateResult, ImportError, ImportConfigJson

  web/
    curation.rs     # curate_references() refactored to call import_from_path()

  cli/
    convert.rs      # ghost convert {pdf, git, crawl}
    reference.rs    # ghost reference {import, update, delete} — reworked
```

**What moves:**

- `fetch_git_manifest()` -> `convert::git` (writes staging dir instead of in-memory vec)
- `fetch_crawl_manifest()` -> `convert::crawl` (same)
- `import_file()` docling call -> `convert::pdf`
- Per-source import functions (`import_git`, `import_crawl`, `import_file`) -> one generic
  `import_from_path()` in `reference_import::import`

**What gets removed:**

- `cli/document.rs` — replaced by `cli/convert.rs`
- `ImportSource` enum — no longer needed, convert and import are separate commands
- Per-source-type import functions — consolidated into generic import

## Web Cache Curation Unification

The reflection agents (`chat-reflection`, `deep-research-reflection`) currently have
their own code path for turning cached web fetches into references:

1. `classify_web_cache()` — match cache files to cited URLs
2. `curate_references()` — raw file move from `.cache/{session_id}/` to `references/`
3. `link_cited_edges()` — create DB records + citation edges

This duplicates the "markdown file → reference on disk + DB record" logic that
`import_from_path()` will own. Unify: `curate_references()` calls `import_from_path()`
for the actual file + DB write, instead of doing it inline.

**What stays in curation:**

- `classify_web_cache()` — deciding which cache files are cited (unchanged)
- `link_cited_edges()` — creating citation graph edges (unchanged)
- Delete logic for uncited files (unchanged)
- Topic resolution from note tags (unchanged)

**What changes:**

- `curate_references()` stops doing its own `fs::rename` + DB insert. Instead, for each
  cited cache file, it calls `import_from_path()` with the resolved topic. This means
  curation gets idempotency, content hashing, and `_import.toml` for free.
- The `import_from_path()` function must accept an optional `source_url` per file (for
  curation, each file has a distinct URL). This is already needed for crawl imports too.

## Skills and Documentation Updates

These must be updated to reflect the new CLI surface:

- **`assets/skills/reference-import/skill.md`** — rewrite to use `ghost convert` +
  `ghost reference import` two-step flow
- **`assets/skills/document-import/skill.md`** — merge into reference-import skill
  (document-import becomes `ghost convert pdf`, then `ghost reference import`)
- **`docs/src/content/docs/knowledge/reference-import.md`** — update CLI examples,
  add convert step, document staging directory

## Future Extensibility

Adding a new format (e.g., `ghost convert epub`) means:

1. Add `convert/epub.rs` with conversion logic
2. Add `Epub` variant to `cli/convert.rs` subcommand enum
3. Done — the import side doesn't change at all
