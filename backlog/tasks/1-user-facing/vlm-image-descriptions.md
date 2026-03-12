# Backlog — VLM Image Descriptions in Document Imports

## Problem

When importing documents (PDF, DOCX) via docling, images are extracted as
`<!-- image -->` placeholders. Diagrams, charts, and photos carry information that is
lost. A VLM could describe them, making the content searchable and useful in context.

## Current State (tested 2026-03-09)

Docling supports `do_picture_description` with a remote VLM via
`picture_description_api`. Tested with granite3.2-vision:2b on Ollama (GTX 1060):

- **Plumbing works**: descriptions stored in JSON `pictures[].annotations[]`
- **Markdown bug**: descriptions don't appear in markdown output (docling #2581). Would
  need post-processing to inject them.
- **Quality is bad**: small cropped images with no context produce vague/wrong
  descriptions. "Beds in a hallway" for a board game zoo layout. "Animated picture with
  a board" for a game board.

The core issue is **lack of context**, not model size. The VLM sees a small cropped
image with no surrounding text. Even a larger model may hallucinate confidently on
ambiguous crops.

## Open Questions

- Does providing surrounding text as context improve descriptions? Docling's prompt is
  just "Describe this image." We could prepend "This image appears in a section about
  {heading}:" — but this requires custom prompt construction per-image.
- Would a **post-docling** approach work better? Extract images + surrounding text
  ourselves, send both to a VLM with richer context. More control but more complexity.
- Is the VLM pipeline (granite-docling-258M) relevant here? It replaces the full
  pipeline, not just image description. On CPU it was slower than standard pipeline. On
  GPU it could be interesting as a single-model alternative.
- What area threshold makes sense? Default 0.05 (5% of page) caught 1/47 images on a
  heavily-illustrated document. 0.01 caught 18/63 but included tiny icons.

## Approach When Revisited

1. Test with context-enriched prompts (heading + caption + surrounding text)
2. Test with a 7B+ VLM (already fits on GTX 1060 via Ollama)
3. Compare quality: with-context vs without-context vs larger model
4. If quality is acceptable, implement post-processing: request JSON from docling,
   extract annotations, replace `<!-- image -->` placeholders in markdown
5. Expose as `--describe-images` flag on `ghost document import`

## Config Design (ready to implement when quality is validated)

```toml
[docling.vlm]
url = "http://localhost:11434/v1/chat/completions"
model = "granite3.2-vision:2b"
prompt = "Describe this image concisely. Focus on the key information it conveys."
concurrency = 1
timeout = 120
area_threshold = 0.05
```
