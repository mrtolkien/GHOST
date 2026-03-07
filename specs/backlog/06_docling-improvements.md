# Backlog — Docling Extraction Improvements

## Performance

Review performance: currently on my homelab (CPU-only) it can take over 10 minutes and
time out.

We need to review what affects docling perf and how we can improve it.

## VLM-Powered Image Descriptions

Currently docling extracts images as placeholders (`image_export_mode: "placeholder"`).
Images in documents (diagrams, charts, photos) carry information that is lost.

Use a VLM (vision-language model) to generate descriptions for extracted images. Docling
supports this natively via `do_picture_description: true` + a VLM pipeline config
(`pipeline: "vlm"`). This requires either a local VLM or an API-compatible endpoint.

### Options

- **Local VLM via docling**: Configure `VlmConvertOptions` with a local model
  (transformers/mlx engine). Requires GPU for reasonable speed.
- **External VLM API**: Point docling at an OpenAI-compatible vision endpoint. Could
  reuse the GHOST's existing provider infrastructure.
- **Post-processing**: Extract images separately, send to a vision model outside
  docling, and inject descriptions back into the markdown. More flexible but more
  complex.

## More Extraction Options

Expose docling conversion options through the CLI and config:

- `do_ocr` / `ocr_engine` — toggle OCR, choose between easyocr/tesseract
- `table_mode` — accurate vs fast table extraction
- `do_formula_enrichment` — LaTeX formula extraction
- `do_code_enrichment` — code block detection
- `page_range` — convert specific pages only (useful for large documents)
- `document_timeout` — per-document timeout override

These could be exposed as:

- CLI flags on `ghost reference import --source file/page`
- Config defaults in `[docling]` section of config.toml
- Both, with CLI overriding config
