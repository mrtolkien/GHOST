# Document reimport CLI command

## Summary

Add a `ghost document reimport` CLI command that re-extracts a previously imported
document using a different method. The GHOST can call this via shell when it detects
poor extraction quality.

## Motivation

Some PDFs (image-only, colored backgrounds, non-Latin scripts) produce garbage through
Docling's standard pipeline. The GHOST needs an escape hatch to re-extract with a
different method — primarily LLM vision.

## CLI interface

```bash
# Re-extract all files in a topic using LLM vision
ghost document reimport --topic personal-care/uploaded-lotion-200ml --method vision

# Re-extract using Docling (e.g., after Docling upgrade or config change)
ghost document reimport --topic personal-care/uploaded-lotion-200ml --method docling

# Re-extract specific pages only (for mixed-quality multi-page PDFs)
ghost document reimport --topic personal-care/uploaded-lotion-200ml --method vision --pages 1,3,5
```

## Methods

- `docling` — re-run Docling conversion (default OCR settings)
- `vision` — render page(s) to PNG, send to the `models.vision` model (falls back to
  `models.default`), get markdown back

## Behavior

1. Locate original file in `references/{topic}/_originals/`
2. Run the selected extraction method
3. Replace the existing `.md` file(s) with new extraction
4. Update DB record (content hash, re-trigger embeddings)

## Config

```toml
[models]
default = "gpt54"
vision = "mimo-v2-omni"  # optional, used for vision extraction; falls back to default
```

## Dependencies

- Depends on the hybrid extraction pipeline (Docling JSON output + per-page quality
  assessment + LLM vision fallback) being implemented first
- `render_page.py` script for PDF page → PNG conversion
