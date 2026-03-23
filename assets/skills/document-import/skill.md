---
name: document-import
description:
  Import documents (PDF, DOCX, XLSX, PPTX, images) into the knowledge base via
  docling-serve. Use when the OPERATOR asks to import a document from a URL or uploaded
  file, when web_fetch returns an unsupported content type, or when you need to import
  non-HTML content as a searchable reference.
---

# Document Import Skill

Import documents (PDF, DOCX, etc.) as topic-scoped references via docling-serve.

## Decision Flow

1. **Search first**: `knowledge_search(query="<topic>", categories=["references"])`. If
   results exist, use them. Done.
2. **URL source**: download the file locally first, then import it:
   - Use `web_fetch` or `curl -L -o uploads/<filename> <url>` to save the file.
   - Then import with
     `ghost document import file --path uploads/<filename> --topic <name>` with
     `background: true`.
3. **File upload**: if the OPERATOR uploaded a file, import with
   `ghost document import file --path uploads/<filename> --topic <name>` with
   `background: true`.
4. **After starting the import**: tell the OPERATOR it's importing, include any other
   pending responses, then **end your turn**. A follow-up turn is triggered
   automatically when the import completes — you'll see the `[shell-command completed]`
   system message. Search the imported refs and answer.

## Why download first?

Many sites serve consent pages, CAPTCHAs, or redirects instead of the actual file.
Downloading first lets you verify the file is real (check size, file type) before
importing. It also keeps the original file on disk in `uploads/` for re-import if
needed.

## CLI Command

```
ghost document import file --path <path> --topic <name>
```

### OPERATOR-facing options (use ONLY when explicitly requested)

These are optimization overrides. **Use defaults unless the OPERATOR asks otherwise.**

| Flag                  | Default  | When to use                               |
| --------------------- | -------- | ----------------------------------------- |
| `--no-ocr`            | OCR on   | OPERATOR says PDF is digital, wants speed |
| `--page-range "1-10"` | full doc | OPERATOR wants specific pages only        |
| `--timeout 900`       | 600s     | OPERATOR needs more time for huge docs    |

Do NOT guess at these options. Do NOT add `--no-ocr` to "speed things up". The OPERATOR
will tell you if they want non-default behavior.

## Running the Import (Background)

Document imports can take 1-2 minutes for a typical PDF. **Always use background mode**:

```json
{
  "command": "ghost document import file --path uploads/rulebook.pdf --topic boardgames/arknova",
  "background": true
}
```

Tell the OPERATOR: _"I'm importing the document in the background — I'll search it once
the import finishes."_ Then **end your turn**.

## Downloading from a URL

When the source is a URL, download the file first:

```json
{
  "command": "curl -L -o uploads/rulebook.pdf 'https://example.com/rulebook.pdf'",
  "background": false
}
```

Verify the download succeeded (check file size, not just exit code — some sites return
HTML error pages with HTTP 200). Then import:

```json
{
  "command": "ghost document import file --path uploads/rulebook.pdf --topic boardgames/arknova",
  "background": true
}
```

## File Import (Uploaded Files)

When the OPERATOR uploads a file, it lands in `uploads/` in the workspace:

```json
{
  "command": "ghost document import file --path uploads/<filename> --topic <topic-name>",
  "background": true
}
```

The original file is preserved in `references/<topic>/_originals/`. After import, clean
up the uploaded file — `uploads/` is a transient inbox:

```json
{
  "command": "rm uploads/<filename>"
}
```

## Post-Import: Enrich the Topic Note

After import, a placeholder note exists at `notes/<topic>/index.md`. Edit it with a
meaningful description — what the document covers, key concepts. This makes the topic
discoverable via semantic search.

## Post-Import Search

```
knowledge_search(query="setup procedure", topic="boardgames/arknova", categories=["references"])
```

## Cleanup

```
ghost reference delete --topic boardgames/arknova
```
