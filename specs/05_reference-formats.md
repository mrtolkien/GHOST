# Backlog — Complex Reference Format Ingestion

## Overview

The PoC knowledge system handles markdown text. Real-world references come in richer
formats that need extraction and indexing: PDFs, images, and large structured data
files.

## Formats

### PDFs

- Extract text content for indexing and search
- Preserve structure (headings, tables, page numbers) where possible
- Handle scanned PDFs (OCR) as a stretch goal
- Store extracted markdown in `knowledge/references/` alongside or instead of the
  original file
- Prior art: many Rust PDF libraries (`pdf-extract`, `lopdf`), or shell out to
  `pdftotext`/`pandoc`

### Images

- Extract text via OCR for indexing
- Generate descriptions via vision model (if available) for semantic search
- Common use cases: screenshots of documentation, diagrams, whiteboard photos, receipts
- Store extracted text/description as markdown alongside the original image
- Prior art: Ollama vision models, Tesseract OCR

### CSV / JSON (large files)

- Large structured data that doesn't fit in a single note
- Options: summarize schema + sample rows, chunk into searchable segments, or index
  column names and values for search
- The GHOST should be able to answer questions about the data without loading the entire
  file into context
- Prior art: DuckDB for SQL-over-CSV/JSON, or chunked indexing into SQLite FTS5

## Design Considerations

- Ingestion should produce standard markdown references that the existing knowledge
  system can index and search — the format-specific logic is a preprocessing step, not a
  new knowledge type
- Large files need chunking strategy aligned with the embeddings system (spec 14)
- Original files should be preserved alongside extracted content for re-processing when
  extraction improves
- Ingestion could be triggered manually (`ghost knowledge ingest <file>`) or
  automatically when files appear in a watched directory
