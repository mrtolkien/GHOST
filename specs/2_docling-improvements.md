# Spec — Docling Improvements

## Status: Design Plan (awaiting review)

## Findings from Testing

### Performance (Ark Nova Rulebook, 20 pages, 9.1 MB, CPU-only)

| Configuration                  | Time    | Notes                               |
| ------------------------------ | ------- | ----------------------------------- |
| EasyOCR + accurate tables      | 465s    | Default docling settings — unusable |
| **RapidOCR** + accurate tables | **75s** | **6x faster than EasyOCR on CPU**   |
| No OCR + accurate tables       | 20s     | Digital PDF only                    |
| No OCR + fast tables           | 19s     | Fastest possible                    |
| RapidOCR, pages 1-5            | 17s     | Scales ~linearly with page count    |

**Key takeaways:**

- **RapidOCR is 6x faster than EasyOCR on CPU.** This alone fixes the timeout problem.
- OCR is the dominant cost (75s with vs 20s without). Table mode is negligible.
- Must use the **async** API (`/v1/convert/source/async` + polling). The sync endpoint
  is inexplicably 4-6x slower (confirmed with warm cache, no queue contention).
- The async endpoint uses JSON with base64-encoded files, not multipart form-data.
- `OMP_NUM_THREADS` / `MKL_NUM_THREADS` must match CPU count (halved time on first fix).

### GPU Acceleration

Not viable on current hardware. See `specs/backlog/3-vlm-image-descriptions.md` for
VLM-specific notes. Revisit when hardware changes (Mac Mini M4 is the clean path).

### VLM Image Descriptions

Tested, quality insufficient. Deferred to backlog:
`specs/backlog/3-vlm-image-descriptions.md`

## Design

### 1. CLI Restructure

Split `ghost reference import --source <type>` into two commands with clap subcommands:

```
ghost reference import <SUBCOMMAND>
  git    --url <url> --topic <name> [--paths dir1,dir2] [--extensions .md,.rs]
  crawl  --url <url> --topic <name> [--max-depth 3] [--max-pages 50]

ghost document import <SUBCOMMAND>
  url    --url <url> --topic <name> [options...]
  file   --path <path> --topic <name> [options...]
```

#### Document import options

These are **OPERATOR-facing overrides**, not GHOST decisions. The GHOST uses defaults
unless the OPERATOR explicitly requests otherwise. The skill must not guess at these.

| Flag                  | Default  | When an OPERATOR would use it                        |
| --------------------- | -------- | ---------------------------------------------------- |
| `--no-ocr`            | OCR on   | OPERATOR knows the PDF is digital and wants speed    |
| `--page-range "1-10"` | full doc | OPERATOR wants specific sections of a large document |
| `--timeout`           | 600s     | OPERATOR needs more time for huge documents          |

**Not exposed** (hardcoded to good defaults):

- OCR engine: always `rapidocr` (6x faster than easyocr, no reason to switch)
- Table mode: always `accurate` (fast mode made no measurable difference)
- Image export: always `placeholder` (VLM descriptions deferred to backlog)

### 2. Config Changes

```toml
[docling]
url = "http://localhost:5001"      # moved from [web] section
timeout = 600                       # seconds, per document
```

Hard break: remove `[web].docling_url` entirely. No deprecated alias — we're pre-alpha,
no backwards compatibility needed.

#### Code changes

- Add `DoclingSettings` / `DoclingConfig` structs to `src/config.rs`
- Add `docling: Option<DoclingSettings>` to `Settings`
- Add `docling: DoclingConfig` to `Config` (resolved with defaults)
- Remove `docling_url` from `WebSettings` / `WebConfig`
- Update `test_config()` to include `DoclingConfig`
- All callers that currently use `config.web.docling_url` switch to `config.docling`

### 3. Async API Client Rewrite

Replace both `convert_file` and `convert_url` in `src/web/docling.rs` with a single
async conversion function. Currently:

- `convert_file()` → POST multipart to `/v1/convert/file` (sync, slow)
- `convert_url()` → POST JSON to `/v1/convert/source` (sync, slow)

Both are replaced by a unified async flow through `/v1/convert/source/async`:

1. **Submit**: POST JSON to `/v1/convert/source/async`
2. **Poll**: GET `/v1/status/poll/{task_id}?wait=5` until `task_status` is terminal
3. **Fetch**: GET `/v1/result/{task_id}` for the conversion result
4. **Timeout**: Cancel after configured timeout, return error

#### Source types in the payload

For **file** sources (base64-encoded):

```json
{
  "sources": [{ "kind": "file", "base64_string": "...", "filename": "..." }],
  "options": {
    "to_formats": ["md"],
    "image_export_mode": "placeholder",
    "pipeline": "standard",
    "do_ocr": true,
    "ocr_engine": "rapidocr",
    "table_mode": "accurate"
  }
}
```

For **URL** sources:

```json
{
  "sources": [{"kind": "http", "url": "..."}],
  "options": { "..." }
}
```

`page_range` is a two-element integer array `[start, end]` (1-indexed, inclusive).
**Omit from payload when not specified** — docling defaults to the full document.
`--page-range "1-10"` parses to `[1, 10]`.

#### API design

```rust
/// Options that callers can override. Hardcoded defaults (ocr_engine, table_mode,
/// image_export_mode) are set internally, not exposed here.
pub struct ConvertOptions {
    pub ocr: bool,           // default: true
    pub page_range: Option<(u32, u32)>,  // None = full document
    pub timeout: Duration,   // from DoclingConfig
}

/// Single entry point for both file and URL conversion.
pub async fn convert(
    config: &DoclingConfig,
    source: DoclingSource,    // File { path } or Url { url }
    options: &ConvertOptions,
) -> Result<String, WebError>
```

### 4. Callers: import_page and import_file

Both `src/reference_import/page.rs` and `src/reference_import/file.rs` currently take
`&WebConfig` solely to access `docling_url`. After the config change:

- Both switch from `&WebConfig` to `&DoclingConfig`
- Both call `docling::convert()` with default `ConvertOptions`
- `import_page` currently has a fallback path: it tries `web::fetch` first, and only
  calls docling on `UnsupportedContentType`. This stays — docling is only for non-text
  content. The `&WebConfig` parameter is removed; `import_page` needs `&DoclingConfig`
  for the fallback and the web fetch config is accessed separately.

The CLI's `--no-ocr`, `--page-range`, `--timeout` flags flow through `ImportSource`
variants and into `ConvertOptions` at the call site.

### 5. Skills

**`reference-import`** (git/crawl — text-based batch imports):

- Decision flow: search first → find git repo → choose paths/extensions → import
- Git import: `gh search repos`, browse repo, `ghost reference import git ...`
- Crawl import: fallback when no git source exists
- Post-import: enrich topic note, search imported refs

**`document-import`** (url/file — docling-powered single documents):

- Decision flow: search first → determine source (URL or uploaded file) → import
- **Use defaults.** Do not add `--no-ocr` or `--page-range` unless the OPERATOR
  explicitly asks. These are optimization knobs, not standard workflow.
- Post-import: same topic note enrichment

Both skills share the post-import guidance (search, topic notes, cleanup). Duplicated
rather than split — the GHOST needs the full decision flow in one place.

The current `reference-import` skill's "page" and "file" source types move to
`document-import`. The `reference-import` skill keeps only git and crawl.

### 6. Implementation Order

1. **Config**: Add `[docling]` section, remove `[web].docling_url`
2. **Async client**: Rewrite `src/web/docling.rs` — unified `convert()` with async flow
3. **Options plumbing**: Add `ConvertOptions`, thread through
   `import_page`/`import_file`
4. **CLI split**: Restructure into `ghost reference import git|crawl` and
   `ghost document import url|file`, add new `DocumentCommand` to top-level clap enum
5. **Skills**: Write `document-import` skill, trim `reference-import` skill

### 7. Recommended compose.yaml

```yaml
docling-serve:
  image: ghcr.io/docling-project/docling-serve:latest
  container_name: docling-serve
  ports:
    - "5001:5001"
  environment:
    - DOCLING_SERVE_ENABLE_UI=1
    - OMP_NUM_THREADS=14 # match CPU count
    - MKL_NUM_THREADS=14
  restart: unless-stopped
```
