# Hybrid PDF Extraction Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps
> use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add per-page quality assessment and LLM vision fallback to the PDF import
pipeline so image-only / garbled pages are re-extracted via a vision model.

**Architecture:** Docling converts PDF → JSON. Rust deserializes, evaluates quality per
page, generates markdown for good pages, and delegates bad pages to a vision LLM via the
existing Provider trait. Final output is a single stitched markdown string — nothing
downstream changes.

**Tech Stack:** Rust (serde, reqwest, tokio), Python (docling, PyMuPDF), existing
Provider trait for LLM calls.

**Spec:** `backlog/tasks/2026-03-25-hybrid-pdf-extraction-design.md`

**Test PDF:** `tmp/1774335624_200mL-1.pdf` (lotion product sheet — image-only,
Japanese text, zero text layer). Copy to test fixtures as needed.

**Reference JSON:** `tmp/docling_full_json.json` — actual DoclingDocument output from the
lotion PDF. Use this to design serde types.

---

### Task 1: Move onboarding templates out of `assets/services/`

Unblock the `build.rs` change so `assets/services/docling/` gets bundled to `$WORKSPACE`.

**Files:**
- Move: `assets/services/docker-compose.searxng.yml` → `src/onboarding/templates/docker-compose.searxng.yml`
- Move: `assets/services/docker-compose.crawl4ai.yml` → `src/onboarding/templates/docker-compose.crawl4ai.yml`
- Move: `assets/services/docker-compose.docling.yml` → `src/onboarding/templates/docker-compose.docling.yml`
- Move: `assets/services/searxng-settings.yml` → `src/onboarding/templates/searxng-settings.yml`
- Modify: `src/onboarding/services.rs:8-11` (update `include_str!` paths)
- Modify: `build.rs:15` (remove `"services"` exclusion)

- [ ] **Step 1: Move the four template files**

```bash
mkdir -p src/onboarding/templates
mv assets/services/docker-compose.searxng.yml src/onboarding/templates/
mv assets/services/docker-compose.crawl4ai.yml src/onboarding/templates/
mv assets/services/docker-compose.docling.yml src/onboarding/templates/
mv assets/services/searxng-settings.yml src/onboarding/templates/
```

- [ ] **Step 2: Update `include_str!` paths in `src/onboarding/services.rs`**

Lines 8-11, change from `../../assets/services/` to `templates/`:

```rust
const SEARXNG_FRAGMENT: &str = include_str!("templates/docker-compose.searxng.yml");
const CRAWL4AI_FRAGMENT: &str = include_str!("templates/docker-compose.crawl4ai.yml");
const DOCLING_FRAGMENT: &str = include_str!("templates/docker-compose.docling.yml");
const SEARXNG_SETTINGS: &str = include_str!("templates/searxng-settings.yml");
```

- [ ] **Step 3: Remove `"services"` exclusion from `build.rs`**

Line 15, change:

```rust
walk_dir(assets_dir, assets_dir, &mut entries, &["services"]);
```

to:

```rust
walk_dir(assets_dir, assets_dir, &mut entries, &[]);
```

Update the comment on line 16-17 to reflect the new state:

```rust
// All assets are bundled to $WORKSPACE/.
```

- [ ] **Step 4: Verify build**

```bash
just ci
```

Expected: all passes. The `convert.py` in `assets/services/docling/` is now bundled.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "refactor: move onboarding templates to src/onboarding/templates/

assets/services/ now bundles entirely to \$WORKSPACE. Docker-compose
templates used by include_str!() move next to the Rust code that uses them."
```

---

### Task 2: Create `render_page.py` script

**Files:**
- Create: `assets/services/docling/render_page.py`

Read the @uv-scripts skill before writing.

- [ ] **Step 1: Write the script**

```python
#!/usr/bin/env python3
# /// script
# requires-python = ">=3.10"
# dependencies = [
#     "pymupdf",
# ]
# ///
"""Render a single PDF page to a PNG image.

Usage:
    uv run render_page.py --path input.pdf --page 1 --output page.png [--dpi 300]
"""

import argparse
import sys
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(
        description="Render a single PDF page to PNG"
    )
    parser.add_argument("--path", required=True, help="Path to the PDF file")
    parser.add_argument(
        "--page", required=True, type=int, help="Page number (1-based)"
    )
    parser.add_argument("--output", required=True, help="Output PNG path")
    parser.add_argument(
        "--dpi", type=int, default=300, help="Resolution in DPI (default: 300)"
    )

    args = parser.parse_args()

    input_path = Path(args.path)
    if not input_path.exists():
        print(f"Error: file not found: {input_path}", file=sys.stderr)
        sys.exit(1)

    output_path = Path(args.output)
    output_path.parent.mkdir(parents=True, exist_ok=True)

    import pymupdf

    doc = pymupdf.open(str(input_path))
    page_index = args.page - 1
    if page_index < 0 or page_index >= len(doc):
        print(
            f"Error: page {args.page} out of range (1-{len(doc)})",
            file=sys.stderr,
        )
        sys.exit(1)

    page = doc[page_index]
    pix = page.get_pixmap(dpi=args.dpi)
    pix.save(str(output_path))
    print(f"Rendered page {args.page}: {output_path.resolve()}")


if __name__ == "__main__":
    main()
```

- [ ] **Step 2: Test it manually against the lotion PDF**

```bash
uv run assets/services/docling/render_page.py --path tmp/1774335624_200mL-1.pdf --page 1 --output /tmp/test_render.png
```

Expected: creates a PNG file, prints path.

- [ ] **Step 3: Commit**

```bash
git add assets/services/docling/render_page.py
git commit -m "feat: add render_page.py for PDF page → PNG conversion"
```

---

### Task 3: Update `convert.py` to output JSON

**Files:**
- Modify: `assets/services/docling/convert.py`

- [ ] **Step 1: Modify `convert.py`**

Replace the markdown export with JSON export. The output file extension changes to
`.json`. The script calls `export_to_dict()` and writes JSON.

Key changes to `main()`:

```python
import json

# ... (existing argparse, converter setup) ...

result = converter.convert(str(input_path))
doc_dict = result.document.export_to_dict()

try:
    output_path.write_text(
        json.dumps(doc_dict, ensure_ascii=False), encoding="utf-8"
    )
except OSError as e:
    print(f"Error writing output file: {e}", file=sys.stderr)
    sys.exit(1)

print(f"Converted: {output_path.resolve()}")
```

Also update the output path default extension in the docstring to mention `.json`.

- [ ] **Step 2: Test locally**

```bash
uv run assets/services/docling/convert.py --path tmp/1774335624_200mL-1.pdf --output /tmp/test_output.json
python3 -c "import json; d=json.load(open('/tmp/test_output.json')); print(d['schema_name'], len(d['texts']), 'texts', len(d['pictures']), 'pictures')"
```

Expected: `DoclingDocument 4 texts 5 pictures` (matching our earlier investigation).

- [ ] **Step 3: Commit**

```bash
git add assets/services/docling/convert.py
git commit -m "feat: convert.py outputs DoclingDocument JSON instead of markdown"
```

---

### Task 4: Add `vision` field to config

**Files:**
- Modify: `src/config.rs`

- [ ] **Step 1: Add `vision` to `ModelsSettings`**

At line 165, after `default`:

```rust
pub struct ModelsSettings {
    pub default: Option<StringOrList>,
    pub vision: Option<String>,
    #[serde(flatten)]
    pub aliases: BTreeMap<String, ModelSettings>,
}
```

- [ ] **Step 2: Add `vision` to `ModelsConfig`**

At line 304, after `default_chain`:

```rust
pub struct ModelsConfig {
    pub default: String,
    pub default_chain: Vec<String>,
    pub vision: Option<String>,
    #[serde(flatten)]
    pub aliases: BTreeMap<String, ModelConfig>,
}
```

- [ ] **Step 3: Wire up resolution in `Config::from_settings()`**

After the `default_chain` validation (around line 480), add vision alias validation.
Note: the local variable is `resolved_aliases` (not `model_aliases`):

```rust
let vision = settings
    .models
    .as_ref()
    .and_then(|m| m.vision.clone());

if let Some(ref alias) = vision {
    if !resolved_aliases.contains_key(alias.as_str()) {
        return Err(ConfigError::UnknownDefaultModelAlias {
            alias: alias.clone(),
        });
    }
}
```

And include `vision` in the `ModelsConfig` construction (around line 484):

```rust
ModelsConfig {
    default: default_model_alias,
    default_chain,
    vision,
    aliases: resolved_aliases,
}
```

Also update `test_config()` (around line 857) — add `vision: None` to the
`ModelsConfig` construction. This function is used by many test files; missing this
will cause compile errors across the test suite.

- [ ] **Step 4: Add test for vision config**

Add a test in `src/config.rs` (near the existing config tests):

```rust
#[test]
fn config_vision_model_alias() {
    let toml = r#"
        [models]
        default = "primary"
        vision = "primary"
        [models.primary]
        provider = "openrouter"
        model = "test-model"
        context_window = 100000
    "#;
    let settings: Settings = toml::from_str(toml).unwrap();
    let config = Config::from_settings(settings).unwrap();
    assert_eq!(config.models.vision, Some("primary".to_string()));
}

#[test]
fn config_vision_model_unknown_alias_fails() {
    let toml = r#"
        [models]
        default = "primary"
        vision = "nonexistent"
        [models.primary]
        provider = "openrouter"
        model = "test-model"
        context_window = 100000
    "#;
    let settings: Settings = toml::from_str(toml).unwrap();
    let err = Config::from_settings(settings).unwrap_err();
    assert!(err.to_string().contains("nonexistent"));
}
```

- [ ] **Step 5: Run tests**

```bash
just ci
```

- [ ] **Step 6: Commit**

```bash
git add src/config.rs
git commit -m "feat: add models.vision config for LLM vision fallback"
```

---

### Task 5: Create `src/docling/` module — error types and document serde

This is the core new module. Start with types that everything else depends on.

**Files:**
- Create: `src/docling/mod.rs`
- Create: `src/docling/error.rs`
- Create: `src/docling/document.rs`
- Modify: `src/main.rs` (add `mod docling;`)

- [ ] **Step 1: Write the error type in `src/docling/error.rs`**

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DoclingError {
    #[error("docling conversion failed: {0}")]
    Conversion(String),

    #[error("docling conversion timed out after {seconds}s")]
    Timeout { seconds: u64 },

    #[error("docling task failed: {detail}")]
    TaskFailed { detail: String },

    #[error("failed to parse DoclingDocument JSON: {0}")]
    Parse(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("render page failed: {0}")]
    RenderPage(String),

    #[error("vision extraction failed: {0}")]
    VisionExtraction(String),
}
```

- [ ] **Step 2: Write DoclingDocument serde types in `src/docling/document.rs`**

Use the actual JSON from `tmp/docling_full_json.json` as reference. Key types:

```rust
use std::collections::BTreeMap;

use serde::Deserialize;

/// Top-level DoclingDocument structure from docling's export_to_dict().
/// Intentionally does NOT use #[serde(deny_unknown_fields)] — Docling's JSON
/// has many fields we don't need, and new versions may add more.
#[derive(Debug, Deserialize)]
pub struct DoclingDocument {
    pub body: BodyNode,
    pub texts: Vec<TextItem>,
    pub pictures: Vec<PictureItem>,
    pub tables: Vec<TableItem>,
    pub groups: Vec<GroupItem>,
    pub pages: BTreeMap<String, PageInfo>,
}

#[derive(Debug, Deserialize)]
pub struct BodyNode {
    pub children: Vec<Ref>,
}

/// A JSON $ref pointer like {"$ref": "#/texts/0"}.
#[derive(Debug, Deserialize)]
pub struct Ref {
    #[serde(rename = "$ref")]
    pub ref_path: String,
}

#[derive(Debug, Deserialize)]
pub struct TextItem {
    pub label: String,
    pub text: String,
    #[serde(default)]
    pub prov: Vec<Provenance>,
    #[serde(default)]
    pub children: Vec<Ref>,
    pub level: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct PictureItem {
    pub label: String,
    #[serde(default)]
    pub prov: Vec<Provenance>,
    #[serde(default)]
    pub children: Vec<Ref>,
}

#[derive(Debug, Deserialize)]
pub struct TableItem {
    pub label: String,
    #[serde(default)]
    pub prov: Vec<Provenance>,
    #[serde(default)]
    pub children: Vec<Ref>,
    // Table grid data — design iteratively from real docling output
    pub data: Option<TableData>,
}

#[derive(Debug, Deserialize)]
pub struct TableData {
    pub grid: Option<Vec<Vec<TableCell>>>,
}

#[derive(Debug, Deserialize)]
pub struct TableCell {
    pub text: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GroupItem {
    pub label: String,
    #[serde(default)]
    pub children: Vec<Ref>,
    #[serde(default)]
    pub prov: Vec<Provenance>,
}

#[derive(Debug, Deserialize)]
pub struct Provenance {
    pub page_no: u32,
    pub bbox: Option<BoundingBox>,
}

#[derive(Debug, Deserialize)]
pub struct BoundingBox {
    pub l: f64,
    pub t: f64,
    pub r: f64,
    pub b: f64,
}

impl BoundingBox {
    pub fn area(&self) -> f64 {
        (self.r - self.l).abs() * (self.t - self.b).abs()
    }
}

#[derive(Debug, Deserialize)]
pub struct PageInfo {
    pub size: Option<PageSize>,
    pub page_no: u32,
}

#[derive(Debug, Deserialize)]
pub struct PageSize {
    pub width: f64,
    pub height: f64,
}

impl PageSize {
    pub fn area(&self) -> f64 {
        self.width * self.height
    }
}
```

- [ ] **Step 3: Write `src/docling/mod.rs`**

```rust
mod document;
mod error;

pub use document::DoclingDocument;
pub use error::DoclingError;
```

- [ ] **Step 4: Add `pub mod docling;` to `src/lib.rs`**

Module declarations live in `src/lib.rs` (not `main.rs`). Add `pub mod docling;`
alongside the other module declarations (alphabetically, between `db` and `embeddings`).

- [ ] **Step 5: Write a unit test that deserializes the lotion PDF JSON**

Copy the lotion PDF's DoclingDocument JSON into a test string constant (or read from a
test fixture file). Verify deserialization succeeds and field counts match.

Add to `src/docling/document.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_lotion_pdf() {
        let json = include_str!("../../tests/fixtures/lotion_docling.json");
        let doc: DoclingDocument = serde_json::from_str(json).unwrap();
        assert_eq!(doc.texts.len(), 4);
        assert_eq!(doc.pictures.len(), 5);
        assert_eq!(doc.pages.len(), 1);
    }
}
```

Copy the clean JSON (without log lines) to `tests/fixtures/lotion_docling.json`.

- [ ] **Step 6: Run tests**

```bash
just ci
```

- [ ] **Step 7: Commit**

```bash
git add src/docling/ src/main.rs tests/fixtures/lotion_docling.json
git commit -m "feat: add docling module with DoclingDocument serde types and error enum"
```

---

### Task 6: Quality assessment

**Files:**
- Create: `src/docling/quality.rs`
- Modify: `src/docling/mod.rs` (add re-export)

- [ ] **Step 1: Write the failing test first**

In `src/docling/quality.rs`:

```rust
use super::document::*;

/// Per-page quality metrics.
#[derive(Debug)]
pub struct PageQuality {
    pub page_no: u32,
    pub text_chars: usize,
    pub text_item_count: usize,
    pub avg_text_length: f64,
    pub picture_area_ratio: f64,
    pub is_good: bool,
}

const MIN_TEXT_CHARS: usize = 50;
const MAX_PICTURE_AREA_RATIO: f64 = 0.5;
const MIN_AVG_TEXT_LENGTH: f64 = 5.0;

/// Assess quality for each page in the document.
pub fn assess_pages(doc: &DoclingDocument) -> Vec<PageQuality> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lotion_pdf_flagged_as_bad() {
        let json = include_str!("../../tests/fixtures/lotion_docling.json");
        let doc: DoclingDocument = serde_json::from_str(json).unwrap();
        let pages = assess_pages(&doc);
        assert_eq!(pages.len(), 1);
        assert!(!pages[0].is_good, "lotion PDF page should be flagged as bad");
        assert!(pages[0].text_chars < MIN_TEXT_CHARS);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test -p ghost docling::quality -- --nocapture
```

Expected: FAIL (todo! panic).

- [ ] **Step 3: Implement `assess_pages`**

```rust
pub fn assess_pages(doc: &DoclingDocument) -> Vec<PageQuality> {
    let mut results = Vec::new();

    for (page_key, page_info) in &doc.pages {
        let page_no = page_info.page_no;
        let page_area = page_info.size.as_ref().map_or(1.0, |s| s.area());

        // Collect text metrics for this page
        let page_texts: Vec<&TextItem> = doc
            .texts
            .iter()
            .filter(|t| t.prov.iter().any(|p| p.page_no == page_no))
            .collect();

        let text_chars: usize = page_texts.iter().map(|t| t.text.len()).sum();
        let text_item_count = page_texts.len();
        let avg_text_length = if text_item_count > 0 {
            text_chars as f64 / text_item_count as f64
        } else {
            0.0
        };

        // Collect picture area for this page
        let picture_area: f64 = doc
            .pictures
            .iter()
            .filter(|p| p.prov.iter().any(|prov| prov.page_no == page_no))
            .filter_map(|p| p.prov.first())
            .filter_map(|prov| prov.bbox.as_ref())
            .map(|bbox| bbox.area())
            .sum();

        let picture_area_ratio = if page_area > 0.0 {
            picture_area / page_area
        } else {
            0.0
        };

        let is_good = !((text_chars < MIN_TEXT_CHARS && picture_area_ratio > MAX_PICTURE_AREA_RATIO)
            || (avg_text_length < MIN_AVG_TEXT_LENGTH && text_item_count > 0));

        results.push(PageQuality {
            page_no,
            text_chars,
            text_item_count,
            avg_text_length,
            picture_area_ratio,
            is_good,
        });
    }

    results.sort_by_key(|p| p.page_no);
    results
}
```

- [ ] **Step 4: Run test to verify it passes**

```bash
cargo test -p ghost docling::quality -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Add `mod quality;` to `src/docling/mod.rs`**

```rust
mod document;
mod error;
mod quality;

pub use document::DoclingDocument;
pub use error::DoclingError;
pub use quality::{PageQuality, assess_pages};
```

- [ ] **Step 6: Commit**

```bash
git add src/docling/quality.rs src/docling/mod.rs
git commit -m "feat: per-page quality assessment for DoclingDocument"
```

---

### Task 7: Markdown generation from DoclingDocument

**Files:**
- Create: `src/docling/markdown.rs`
- Modify: `src/docling/mod.rs`

- [ ] **Step 1: Write failing tests first**

```rust
use super::document::*;

/// Generate markdown for a specific page from the DoclingDocument.
/// If `page_no` is None, generates markdown for all pages.
pub fn generate_markdown(doc: &DoclingDocument, page_no: Option<u32>) -> String {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn section_header_becomes_heading() {
        let json = r#"{
            "body": {"children": [{"$ref": "#/texts/0"}]},
            "texts": [{"label": "section_header", "text": "Hello", "prov": [{"page_no": 1}], "children": [], "level": 2}],
            "pictures": [], "tables": [], "groups": [],
            "pages": {"1": {"page_no": 1, "size": {"width": 100, "height": 100}}}
        }"#;
        let doc: DoclingDocument = serde_json::from_str(json).unwrap();
        let md = generate_markdown(&doc, None);
        assert_eq!(md.trim(), "## Hello");
    }

    #[test]
    fn text_becomes_paragraph() {
        let json = r#"{
            "body": {"children": [{"$ref": "#/texts/0"}]},
            "texts": [{"label": "text", "text": "Some paragraph.", "prov": [{"page_no": 1}], "children": []}],
            "pictures": [], "tables": [], "groups": [],
            "pages": {"1": {"page_no": 1, "size": {"width": 100, "height": 100}}}
        }"#;
        let doc: DoclingDocument = serde_json::from_str(json).unwrap();
        let md = generate_markdown(&doc, None);
        assert_eq!(md.trim(), "Some paragraph.");
    }

    #[test]
    fn picture_becomes_placeholder() {
        let json = r#"{
            "body": {"children": [{"$ref": "#/pictures/0"}]},
            "texts": [],
            "pictures": [{"label": "picture", "prov": [{"page_no": 1, "bbox": {"l": 0, "t": 100, "r": 100, "b": 0}}], "children": []}],
            "tables": [], "groups": [],
            "pages": {"1": {"page_no": 1, "size": {"width": 100, "height": 100}}}
        }"#;
        let doc: DoclingDocument = serde_json::from_str(json).unwrap();
        let md = generate_markdown(&doc, None);
        assert_eq!(md.trim(), "<!-- image -->");
    }

    #[test]
    fn page_filter_only_returns_requested_page() {
        let json = r#"{
            "body": {"children": [{"$ref": "#/texts/0"}, {"$ref": "#/texts/1"}]},
            "texts": [
                {"label": "text", "text": "Page one.", "prov": [{"page_no": 1}], "children": []},
                {"label": "text", "text": "Page two.", "prov": [{"page_no": 2}], "children": []}
            ],
            "pictures": [], "tables": [], "groups": [],
            "pages": {
                "1": {"page_no": 1, "size": {"width": 100, "height": 100}},
                "2": {"page_no": 2, "size": {"width": 100, "height": 100}}
            }
        }"#;
        let doc: DoclingDocument = serde_json::from_str(json).unwrap();
        let md = generate_markdown(&doc, Some(2));
        assert!(!md.contains("Page one"));
        assert!(md.contains("Page two"));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test -p ghost docling::markdown -- --nocapture
```

- [ ] **Step 3: Implement `generate_markdown`**

Walk `body.children`, resolve `$ref` pointers, filter by `page_no` if specified, emit
markdown based on label. Handle the `#/texts/N`, `#/pictures/N`, `#/tables/N`,
`#/groups/N` ref patterns.

Key logic: parse the `$ref` string (e.g., `#/texts/0`) to get collection name and index,
look up the item, check its `prov[].page_no`, emit based on label.

For groups: recurse into their children.

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test -p ghost docling::markdown -- --nocapture
```

- [ ] **Step 5: Add re-export in `src/docling/mod.rs`**

```rust
mod markdown;
pub use markdown::generate_markdown;
```

- [ ] **Step 6: Commit**

```bash
git add src/docling/markdown.rs src/docling/mod.rs
git commit -m "feat: markdown generation from DoclingDocument tree"
```

---

### Task 8: Move docling conversion to `src/docling/convert.rs`

Migrate the existing conversion logic from `src/web/docling.rs` to the new module,
updating it to return `DoclingDocument` instead of a markdown string.

**Files:**
- Create: `src/docling/convert.rs`
- Modify: `src/web/mod.rs` (remove `pub mod docling;`)
- Delete: `src/web/docling.rs`
- Modify: `src/web/types.rs` (remove Docling error variants)
- Modify: `src/reference_import/file.rs` (update imports)
- Modify: `src/reference_import/types.rs` (add `#[from] DoclingError`)
- Modify: `src/docling/mod.rs` (add re-exports)

- [ ] **Step 1: Create `src/docling/convert.rs`**

Copy the conversion logic from `src/web/docling.rs`. Key changes:

- Return type: `Result<DoclingDocument, DoclingError>` (was `Result<String, WebError>`)
- HTTP backend: request `to_formats: ["json"]`, extract `json_content` from response,
  deserialize as `DoclingDocument`
- Script backend: read `.json` output file (change `output.md` → `output.json` in temp
  path), deserialize as `DoclingDocument`
- Script discovery: use `workspace.join("services/docling/convert.py")` (the function
  now takes `workspace: &Path` as a parameter). Remove `find_convert_script()` entirely.
  Note: in development, `bootstrap_workspace()` must have run to populate the workspace
  with bundled assets. The CLI already calls this (line 48 of `document.rs`).
- Remove `extract_markdown_from_response()` — no longer needed
- Replace all `WebError::Docling*` with `DoclingError::*`

The `convert()` function signature becomes:

```rust
pub async fn convert(
    config: &DoclingConfig,
    workspace: &Path,
    source: DoclingSource<'_>,
    options: &ConvertOptions,
) -> Result<DoclingDocument, DoclingError>
```

- [ ] **Step 2: Update `src/docling/mod.rs` with re-exports**

```rust
mod convert;
mod document;
mod error;
mod markdown;
mod quality;

pub use convert::{ConvertOptions, DoclingSource, convert};
pub use document::DoclingDocument;
pub use error::DoclingError;
pub use markdown::generate_markdown;
pub use quality::{PageQuality, assess_pages};
```

- [ ] **Step 3: Remove docling from `src/web/`**

Delete `src/web/docling.rs`. Remove `pub mod docling;` from `src/web/mod.rs`.
Remove `Docling`, `DoclingTimeout`, `DoclingTaskFailed` variants from
`src/web/types.rs`. Run `grep -r 'WebError::Docling' src/` to verify no other
references remain.

- [ ] **Step 4: Add `#[from] DoclingError` to `ImportError`**

In `src/reference_import/types.rs`, add:

```rust
#[error("docling error: {0}")]
Docling(#[from] crate::docling::DoclingError),
```

- [ ] **Step 5: Update `src/reference_import/file.rs`**

Change the import and call site. The function now calls
`crate::docling::convert()` which returns a `DoclingDocument`. Then use
`crate::docling::generate_markdown(&doc, None)` to get the full markdown.

The `workspace` parameter is already available. Update the call:

```rust
let doc = crate::docling::convert(
    docling_config,
    workspace,
    crate::docling::DoclingSource::File { path: &source_path },
    &convert_opts,
)
.await?;

let markdown = crate::docling::generate_markdown(&doc, None);
```

- [ ] **Step 6: Verify build**

```bash
just ci
```

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "refactor: move docling to src/docling/ module, return DoclingDocument JSON

- HTTP backend requests to_formats: [\"json\"], parses json_content
- Script backend reads .json output, deserializes DoclingDocument
- Markdown generated in Rust from document tree
- find_convert_script() replaced with workspace path lookup
- WebError::Docling* variants removed, DoclingError used instead"
```

---

### Task 9: Wire up the LLM vision fallback

This is the core feature: bad pages get re-extracted via a vision model.

**Files:**
- Create: `src/docling/vision.rs`
- Modify: `src/docling/mod.rs`
- Modify: `src/reference_import/file.rs`
- Modify: `src/cli/document.rs`

- [ ] **Step 1: Write `src/docling/vision.rs`**

```rust
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use crate::providers::types::{
    ChatMessage, ChatRequest, ContentBlock, Provider, Role,
};

use super::DoclingError;

const VISION_PROMPT: &str = "\
Extract ALL text from this document page. Respond in markdown format.

Rules:
- Preserve the reading order and document structure (headings, lists, tables).
- Render tables as markdown tables.
- For images, photos, logos, or diagrams: describe them as \
  [Image: detailed description of what the image shows].
- Do not skip any text, including fine print, footnotes, and labels.
- Do not add any commentary or explanation. Output only the document content.";

/// Render a PDF page to PNG, send to a vision model, return markdown.
pub async fn extract_page_with_vision(
    provider: &Arc<dyn Provider>,
    model: &str,
    workspace: &Path,
    pdf_path: &Path,
    page_no: u32,
) -> Result<String, DoclingError> {
    let (png_path, _tmp_guard) = render_page(workspace, pdf_path, page_no).await?;

    let request = ChatRequest {
        model: model.to_string(),
        messages: vec![ChatMessage {
            role: Role::User,
            content: vec![
                ContentBlock::Image {
                    path: png_path.to_string_lossy().to_string(),
                    mime_type: "image/png".to_string(),
                    filename: format!("page_{page_no}.png"),
                },
                ContentBlock::Text {
                    text: VISION_PROMPT.to_string(),
                },
            ],
        }],
        ..Default::default()
    };

    let response = provider
        .chat(request)
        .await
        .map_err(|e| DoclingError::VisionExtraction(e.to_string()))?;

    // Extract text from response — ChatResponse has .content directly (no .message)
    let text: String = response
        .content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("");

    // _tmp_guard drops here, cleaning up the temp directory and PNG
    drop(_tmp_guard);

    if text.is_empty() {
        return Err(DoclingError::VisionExtraction(
            "vision model returned empty response".into(),
        ));
    }

    Ok(text)
}

/// Returns (png_path, _temp_dir_guard). The guard must be kept alive until
/// the PNG is no longer needed — dropping it deletes the temp directory.
async fn render_page(
    workspace: &Path,
    pdf_path: &Path,
    page_no: u32,
) -> Result<(std::path::PathBuf, tempfile::TempDir), DoclingError> {
    let script = workspace.join("services/docling/render_page.py");
    if !script.exists() {
        return Err(DoclingError::RenderPage(format!(
            "render_page.py not found at {}",
            script.display()
        )));
    }

    // Keep TempDir guard alive so it auto-cleans on drop.
    // Return both the path and the guard to prevent leaking temp directories.
    let tmp_dir = tempfile::tempdir().map_err(DoclingError::Io)?;
    let output_path = tmp_dir.path().join(format!("page_{page_no}.png"));

    let result = tokio::process::Command::new("uv")
        .arg("run")
        .arg(&script)
        .arg("--path")
        .arg(pdf_path)
        .arg("--page")
        .arg(page_no.to_string())
        .arg("--output")
        .arg(&output_path)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .await
        .map_err(|e| DoclingError::RenderPage(format!("failed to spawn uv: {e}")))?;

    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        return Err(DoclingError::RenderPage(stderr.to_string()));
    }

    Ok((output_path, tmp_dir))
}
```

Update the caller in `extract_page_with_vision` to keep the guard:
```rust
let (png_path, _tmp_guard) = render_page(workspace, pdf_path, page_no).await?;
// _tmp_guard keeps the temp dir alive until this scope exits
```

Remove the manual `tokio::fs::remove_file` cleanup — the TempDir drop handles it.

Note: the exact `ChatRequest` fields depend on the current struct definition. The
implementer must check `src/providers/types.rs` for `ChatRequest` fields and adjust.
The key fields are `model` and `messages`. Set other fields to defaults.

- [ ] **Step 2: Write the hybrid orchestrator**

Add to `src/docling/mod.rs` or a new `src/docling/hybrid.rs`:

```rust
/// Convert a PDF with quality assessment and optional LLM vision fallback.
/// Returns the final stitched markdown.
pub async fn convert_hybrid(
    config: &DoclingConfig,
    workspace: &Path,
    source: DoclingSource<'_>,
    options: &ConvertOptions,
    vision_provider: Option<Arc<dyn Provider>>,
    vision_model: &str,
) -> Result<String, DoclingError> {
    let doc = convert(config, workspace, source, options).await?;
    let page_qualities = assess_pages(&doc);

    let bad_pages: Vec<u32> = page_qualities
        .iter()
        .filter(|p| !p.is_good)
        .map(|p| p.page_no)
        .collect();

    if bad_pages.is_empty() || vision_provider.is_none() {
        // All good or no vision provider — use Docling markdown for everything
        return Ok(generate_markdown(&doc, None));
    }

    let provider = vision_provider.unwrap();
    let mut page_markdowns = Vec::new();

    for pq in &page_qualities {
        if pq.is_good {
            page_markdowns.push(generate_markdown(&doc, Some(pq.page_no)));
        } else {
            // Need the original PDF path for rendering
            let DoclingSource::File { path } = &source else {
                // URL sources can't render pages — fall back to Docling markdown
                page_markdowns.push(generate_markdown(&doc, Some(pq.page_no)));
                continue;
            };
            match vision::extract_page_with_vision(
                &provider, vision_model, workspace, path, pq.page_no,
            ).await {
                Ok(md) => page_markdowns.push(md),
                Err(e) => {
                    tracing::warn!(page = pq.page_no, error = %e, "vision fallback failed, using Docling output");
                    page_markdowns.push(generate_markdown(&doc, Some(pq.page_no)));
                }
            }
        }
    }

    Ok(page_markdowns.join("\n\n"))
}
```

- [ ] **Step 3: Update `src/reference_import/file.rs`**

Add `vision_provider: Option<Arc<dyn Provider>>` parameter. Replace the direct
`docling::convert()` + `generate_markdown()` call with `docling::convert_hybrid()`.

The vision model name comes from config — passed through or resolved in the CLI.
Add a `vision_model: Option<&str>` parameter or resolve it before calling.

- [ ] **Step 4: Update `src/cli/document.rs`**

Resolve the vision provider from config and pass it to `import_file()`:

```rust
// After loading config — resolve both provider and model name
let vision_alias = config.models.vision.as_deref()
    .unwrap_or(&config.models.default);
let vision_provider = crate::providers::provider_for_alias(&config, Some(vision_alias)).ok();
let vision_model = config.models.aliases.get(vision_alias)
    .map(|m| m.model.clone());
```

Pass `vision_provider` and `vision_model.as_deref()` to `import_file()`.
The `import_file()` function threads these through to `convert_hybrid()`.

- [ ] **Step 5: Run full build**

```bash
just ci
```

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat: LLM vision fallback for bad PDF pages

When Docling produces poor results for a page (low text, high image coverage),
the page is rendered to PNG and sent to a vision model for extraction.
Model configured via models.vision (falls back to models.default)."
```

---

### Task 10: Live tests

**Files:**
- Create: `tests/fixtures/lotion_docling.json` (if not already created in Task 5)
- Copy: `tmp/1774335624_200mL-1.pdf` → `tests/fixtures/lotion.pdf`
- Create: `tests/docling_live.rs` (new top-level test file for docling pipeline tests)

Read the @testing and @e2e-testing skills before writing tests.

- [ ] **Step 1: Copy test fixtures**

```bash
cp tmp/1774335624_200mL-1.pdf tests/fixtures/lotion.pdf
```

- [ ] **Step 2: Write live test with MockProvider**

`--features live-tests` — requires a local Docling instance but not a paid LLM.

Test that: Docling returns JSON, quality check flags the page as bad, MockProvider is
called with a `ContentBlock::Image`, and the mock's response is used in the final
markdown.

- [ ] **Step 3: Write live test with real vision model**

`--features live-tests-llms` — requires both Docling and a real vision model.

Test that: the full pipeline produces markdown containing the actual Japanese product
text (check for `ヘパリン類似物質` — the key ingredient name, or `健栄製薬` — the
manufacturer name). This proves the entire pipeline works on the exact PDF that
triggered this work.

- [ ] **Step 4: Run live tests**

```bash
cargo test --features live-tests docling -- --nocapture
cargo test --features live-tests-llms docling -- --nocapture
```

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "test: live tests for hybrid PDF extraction pipeline"
```

---

### Task 11: Final cleanup and verification

- [ ] **Step 1: Run full CI**

```bash
just ci
```

- [ ] **Step 2: Verify the lotion PDF end-to-end via CLI**

```bash
# Create a test workspace or use existing
ghost document import file --path tmp/1774335624_200mL-1.pdf --topic test/lotion
cat ~/GHOST/references/test/lotion/1774335624_200mL-1.md
```

Verify the output contains readable Japanese text, not garbled `Fの L。`.

- [ ] **Step 3: Clean up tmp/ test files**

Remove `tmp/test_docling_*.py`, `tmp/docling_*.md`, `tmp/openrouter_*.md` and other
investigation artifacts that are no longer needed.

- [ ] **Step 4: Commit any remaining changes**

```bash
git add -A
git commit -m "chore: cleanup investigation artifacts"
```
