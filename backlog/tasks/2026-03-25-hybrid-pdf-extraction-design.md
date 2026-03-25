# Hybrid PDF extraction: Docling + LLM vision fallback

## Problem

Docling's standard pipeline (layout detection + RapidOCR) fails on image-only PDFs,
colored-background documents, and non-Latin scripts. The lotion product sheet
(`1774335624_200mL-1.pdf`) is a concrete example: zero fonts, zero text objects, pure
raster images. Docling produces `<!-- image --> Fの L。` — garbled, unusable output.

LLM vision models (Claude Opus, GPT-5.4, mimo-v2-omni) extract this content perfectly
when given the page as a PNG.

## Goal

Make PDF import autonomous and reliable by adding per-page quality assessment and an LLM
vision fallback for pages that Docling fails on. The rest of the system (DB, embeddings,
file watcher, references) sees no change — it still receives a single markdown string per
imported file.

## Approach: Docling-first with per-page LLM fallback

Always run Docling first. Evaluate quality per page from structured output. Re-extract
bad pages via LLM vision. Stitch into a single markdown file.

```
PDF in
  │
  ├─ Docling (single call, local script or HTTP)
  │   → returns: DoclingDocument JSON
  │
  ├─ Rust: deserialize JSON, per-page quality assessment
  │   → classifies each page as "good" or "bad"
  │
  ├─ Good pages: Rust generates markdown from DoclingDocument tree
  ├─ Bad pages: render_page.py → PNG → Provider::chat() → markdown
  │
  └─ Stitch all pages in order → single markdown string → disk + DB
```

### Why Docling-first (not pre-classification)

- Docling gives us the structured JSON we need for quality assessment — no separate
  inspection step required.
- Works for mixed PDFs: some pages may have good text layers while others are scanned.
- Docling is fast for text-heavy pages.
- We can add a pre-classification optimization later if Docling latency becomes an issue
  for fully image-only PDFs.

## Docling output contract

Both backends return DoclingDocument JSON. Rust generates all markdown.

**Local script** (`convert.py`): outputs DoclingDocument JSON via `export_to_dict()`.
Simpler contract than today (was: markdown string, now: JSON).

**HTTP backend** (`convert_http()`): requests `to_formats: ["json"]`. The docling-serve
API already returns `json_content` containing the full DoclingDocument. Verified against
the server at `192.168.1.10:5001`.

**Rust owns markdown generation.** It walks the DoclingDocument body tree
(`body.children` → resolve `$ref` pointers → emit markdown based on item `label`). Every
item has `prov[].page_no`, so per-page filtering is trivial.

Label-to-markdown mapping:

| Label            | Markdown output              |
| ---------------- | ---------------------------- |
| `title`          | `# text`                     |
| `section_header` | `#` repeated by level + text |
| `text`           | text + blank line            |
| `paragraph`      | text + blank line            |
| `list_item`      | `- text`                     |
| `table`          | markdown table from grid     |
| `code`           | fenced code block            |
| `picture`        | `<!-- image -->`             |
| `formula`        | `$latex$`                    |
| groups           | recurse into children        |

## Per-page quality assessment

Metrics computed per page from the DoclingDocument JSON:

- `text_chars`: total characters across all text items on the page
- `picture_area_ratio`: sum of picture bounding box areas / page area
- `avg_text_length`: mean characters per text item
- `text_item_count`: number of text items on the page

A page is flagged as **bad** if:

- `text_chars < 50` AND `picture_area_ratio > 0.5` — very little text, mostly images
- OR `avg_text_length < 5` AND `text_item_count > 0` — OCR fired but produced garbled
  fragments

Thresholds are constants in Rust. Informed by industry heuristics (MinerU uses 50
chars/page, Marker uses 30% alphanumeric ratio).

For the lotion PDF: 4 text items, 20 total chars, avg 5 chars/item, 5 pictures covering
~80% of the page → flagged immediately.

## LLM vision fallback

When a page is flagged as bad:

1. **Render to PNG**: shell out to `render_page.py` (PyMuPDF). Takes
   `(pdf_path, page_number, output_path)`, writes a 300dpi PNG.

2. **Call the Provider**: use `Provider::chat()` with `ContentBlock::Image` (already
   supported by all providers — Anthropic, OpenAI-compatible, OpenRouter).

3. **Prompt** (informed by olmOCR, Marker, swift-ocr best practices):

```
Extract ALL text from this document page. Respond in markdown format.

Rules:
- Preserve the reading order and document structure (headings, lists, tables).
- Render tables as markdown tables.
- For images, photos, logos, or diagrams: describe them as
  [Image: detailed description of what the image shows].
- Do not skip any text, including fine print, footnotes, and labels.
- Do not add any commentary or explanation. Output only the document content.
```

4. **Model selection**: uses `models.vision` alias if configured, falls back to
   `models.default`.

### Provider plumbing

`import_file()` gains an `Option<Arc<dyn Provider>>` parameter:

```rust
pub async fn import_file(
    db: &GhostDb,
    workspace: &Path,
    docling_config: &DoclingConfig,
    config: &ImportConfig,
    vision_provider: Option<Arc<dyn Provider>>,
) -> Result<ImportResult, ImportError>
```

- `None` → Docling-only, Rust generates markdown from JSON, no LLM fallback (like today
  but with the new JSON contract)
- `Some(provider)` → hybrid pipeline: quality-assess each page, render bad pages to PNG,
  call provider, stitch

The orchestration (quality check → render → LLM call → stitch) lives in a new function
in `src/docling/` (e.g., `docling::convert_hybrid()`). `import_file()` delegates to it
instead of calling `docling::convert()` directly.

The CLI resolves the vision provider from config and passes it in:

- `models.vision` is set → resolve that alias into a provider
- `models.vision` is not set → use `models.default` as the vision provider
- A vision provider is always constructed when a default model exists. Pass `None` only
  in tests or if no model is configured at all.

This means LLM fallback is always available but only fires when pages fail the quality
check — it costs nothing for PDFs where Docling succeeds.

## Configuration

```toml
[models]
default = "gpt54"
vision = "mimo-v2-omni"  # optional — LLM vision fallback for PDF import
                          # falls back to models.default if not set
```

The `vision` key is a model alias name (string), resolved the same way as `default`.
Implementation note: `ModelsSettings` uses `#[serde(flatten)]` for aliases, so `vision`
must be added as a **named field** on `ModelsSettings` (like `default`) — not left to
the flattened `BTreeMap`. A bare string would fail to deserialize as `ModelSettings`.

```rust
// Raw TOML deserialization
pub struct ModelsSettings {
    pub default: Option<StringOrList>,
    pub vision: Option<String>,  // model alias for vision fallback
    #[serde(flatten)]
    pub aliases: BTreeMap<String, ModelSettings>,
}

// Resolved config (validated)
pub struct ModelsConfig {
    // ... existing fields ...
    pub vision: Option<String>,  // validated alias name
}
```

During `Config::from_settings()`, validate that the `vision` alias exists in the aliases
map (same pattern as the `default` chain validation).

## File changes

### Modified files

| File                               | Change                                                                                                          |
| ---------------------------------- | --------------------------------------------------------------------------------------------------------------- |
| `assets/services/docling/convert.py` | Output DoclingDocument JSON instead of markdown                                                                 |
| `src/reference_import/file.rs`     | Gains `Option<Arc<dyn Provider>>` param; orchestrates quality check → fallback → stitch                        |
| `src/config.rs`                    | Add optional `vision` field to models config                                                                    |
| `src/cli/document.rs`              | Resolve vision provider and pass it to import                                                                   |

### New files

| File                                    | Purpose                                                   |
| --------------------------------------- | --------------------------------------------------------- |
| `src/docling/mod.rs`                    | Re-exports, `DoclingSource`, `ConvertOptions`             |
| `src/docling/convert.rs`               | Local script + HTTP backend (both return JSON)            |
| `src/docling/document.rs`              | `DoclingDocument` serde types                             |
| `src/docling/markdown.rs`              | Tree walk → per-page markdown generation                  |
| `src/docling/quality.rs`               | Per-page quality assessment                               |
| `assets/services/docling/render_page.py` | PyMuPDF: `(pdf, page_no, output)` → 300dpi PNG           |

### Removed files

| File              | Reason                          |
| ----------------- | ------------------------------- |
| `src/web/docling.rs` | Moved to `src/docling/` module |

### What doesn't change

- Provider trait — `ContentBlock::Image` already works
- DB schema — still one markdown string per reference
- Embeddings pipeline — still hashes the final markdown
- File watcher — still picks up `.md` files
- Everything downstream of "here's a markdown string" is untouched

## Output

One PDF → one `.md` file → one DB reference. Same as today. The per-page logic is
internal to the extraction pipeline. Human readability of the final markdown is
preserved.

## Error handling

The `src/docling/` module gets its own `DoclingError` enum, replacing the current
`WebError::Docling*` variants. `ImportError` gains a `#[from] DoclingError` variant
instead of the current `ImportError::Fetch(String)` for docling failures.

## `no_ocr` interaction

When `no_ocr = true`, Docling skips OCR entirely, producing even less text for
image-heavy pages. These pages are more likely to be flagged as "bad" and sent to the
LLM fallback. This is intentional — the LLM handles image-only pages regardless of OCR
settings.

## Script bundling and discovery

`convert.py` currently lives in `assets/services/docling/` but is excluded from the
workspace bundle — `build.rs` skips all of `assets/services/` because it contains
compile-time onboarding templates (docker-compose files, searxng settings). The runtime
uses a brittle binary-relative path hack (`find_convert_script()`) to locate it.

**Fix**: Move onboarding templates out of `assets/services/` into
`src/onboarding/templates/`:

```
src/onboarding/
├── templates/
│   ├── docker-compose.searxng.yml
│   ├── docker-compose.crawl4ai.yml
│   ├── docker-compose.docling.yml
│   └── searxng-settings.yml
├── services.rs    # include_str!("templates/docker-compose.searxng.yml")
└── ...
```

Then remove the `"services"` exclusion from `build.rs`. `assets/` becomes purely
"things that go to `$WORKSPACE/`", no exceptions. `convert.py` and `render_page.py`
naturally end up at `$WORKSPACE/services/docling/`.

**Runtime lookup**: Replace `find_convert_script()` with
`workspace.join("services/docling/convert.py")`. No more binary-relative path
resolution.

## Testing

- **Unit tests** for quality assessment: synthetic `DoclingDocument` JSON → verify page
  classification. Deterministic, no external dependencies.
- **Unit tests** for markdown generation: small `DoclingDocument` JSON → verify markdown
  output for each label type.
- **Live test** (`--features live-tests`): full pipeline against the lotion PDF
  (`1774335624_200mL-1.pdf`) with a real local Docling instance. Verify Docling returns
  JSON, quality assessment flags the page as bad, and the `MockProvider` is called with
  a `ContentBlock::Image`.
- **Live test** (`--features live-tests-llms`): full end-to-end against the lotion PDF
  with both a real Docling instance and a real vision model. Verify the final markdown
  contains the actual product text (ingredient list, usage instructions). This is the
  critical test — it proves the entire pipeline works on the exact PDF that triggered
  this work.

## Implementation notes

The `DoclingDocument` serde types are non-trivial. The JSON uses `$ref` pointers
(e.g., `#/texts/0`) that need resolution, nested children, and ~30 label variants.
Design the serde types iteratively against actual Docling output — a representative
JSON sample from the lotion PDF is in `tmp/` from the investigation session.

The `convert.py` output file should use `.json` extension (was `.md`) to avoid
confusion. `convert_script()` return type changes from `Result<String, _>` to
`Result<DoclingDocument, DoclingError>`.

The HTTP backend switches from `to_formats: ["md"]` to `to_formats: ["json"]` and
reads `json_content` instead of `md_content`. The `extract_markdown_from_response()`
function is removed.

## Future work (backlog)

`ghost document reimport` CLI command — re-extract a previously imported document using
a different method. Spec in `backlog/tasks/9-extras/document-reimport-cli.md`.
