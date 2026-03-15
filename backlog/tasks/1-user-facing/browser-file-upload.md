# Browser File Upload

## Status: FUTURE (first priority after browser tool MVP)

## Motivation

The GHOST needs to upload files through web forms — submitting documents, attaching
images, importing data. Without this, any workflow that involves a file input is a dead
end.

## Implementation

New `upload` action on the `browser` tool:

```json
{"action": "upload", "ref": "e7", "path": "documents/report.pdf"}
```

CDP command: `DOM.setFileInputFiles` — takes a backend node ID and a list of file paths.
Simple, well-supported, ~20 LoC.

## The Hard Part: Container Boundary

`DOM.setFileInputFiles` expects file paths **relative to the Chrome process**, not Ghost.
Chrome runs in a Docker sidecar. The GHOST's workspace files aren't visible to Chrome
unless we solve this.

Options:

1. **Shared volume mount** — mount `$WORKSPACE` (or a subdirectory like
   `$WORKSPACE/.cache/browser/uploads/`) into the Chrome container. Ghost copies/symlinks
   files there before upload. Chrome sees them at a known path.

2. **Base64 via CDP** — use `Runtime.evaluate` to programmatically set files on the
   input element via JavaScript `File` and `DataTransfer` APIs. Avoids volume mount
   entirely but is hackier and may not work on all sites (some validate file inputs).

3. **Named pipe / tmpfs** — shared tmpfs between containers. Ghost writes file, Chrome
   reads it, file is cleaned up. Avoids persistent volume but adds Docker config.

**Recommendation**: Option 1 (shared volume). Simplest, most reliable. Add a
`/uploads` mount to the Chrome sidecar in docker-compose:

```yaml
services:
  chrome:
    volumes:
      - ghost-uploads:/uploads
```

Ghost copies the file to the upload directory, calls `DOM.setFileInputFiles` with
`/uploads/filename`, cleans up after.

## Schema Addition

```json
"path": {
  "type": "string",
  "description": "Workspace-relative path to file for 'upload' action."
}
```
