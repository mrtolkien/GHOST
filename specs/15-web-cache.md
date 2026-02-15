# 15 — Web Cache and Curation

## Overview

Web results fetched during conversations are automatically cached in
`$WORKSPACE/.web-cache/`. During reflection jobs, the GHOST curates these cached files
into proper reference topics or discards them.

This pattern ensures no information slips away during conversations — the GHOST can focus
on answering the OPERATOR, knowing reflection will organize everything afterward.

## Cache Flow

```
Chat: GHOST calls web_fetch
    ↓
Content saved to .web-cache/ with metadata
    ↓
Reflection job runs
    ↓
GHOST reviews each cached file:
  - Useful → move to reference topic with reference_manage
  - Garbage → delete or let auto-clear handle it
    ↓
.web-cache/ is cleared after successful reflection
```

## Cache File Format

```
.web-cache/
├── 2025-02-15T14-30-00_example-com_page-title.md
└── 2025-02-15T14-35-00_docs-rs_surrealdb.md
```

Filename format: `{timestamp}_{domain}_{slug}.md`

File content:

```markdown
---
url: https://example.com/article
fetched_at: 2025-02-15T14:30:00Z
---

# Article Title

[extracted content...]
```

## Reflection Integration

The reflection prompt includes the list of web cache files:

```markdown
### Your cached web results:

{{ web_cache_files }}
```

The GHOST uses these reflection tools to curate:

- `reference_manage(action="move", cache_file=".web-cache/file.md", target_topic="topic", target_filename="name.md")` — Move to a reference topic
- `reference_manage(action="delete", cache_file=".web-cache/file.md")` — Delete garbage
- Or skip — the directory is auto-cleared after reflection completes successfully

## Auto-Clear

After a successful reflection run (status = "ok"), the `.web-cache/` directory is
cleared. This means:

- If reflection fails, cached files are preserved for the next attempt
- Files the GHOST skipped are also cleared (assumed intentionally skipped)
- The GHOST should curate everything worth keeping before finishing reflection

## What Gets Cached

- Successful `web_fetch` responses (HTTP 2xx) → cached
- Failed fetches (4xx, 5xx, timeouts) → NOT cached
- `web_search` results → NOT cached (they're just snippets, not full content)

## Acceptance Criteria

- Successful `web_fetch` calls save content to `.web-cache/`
- Cache files include URL and timestamp metadata
- Failed fetches do not create cache files
- Reflection prompt includes the web cache file list
- `reference_manage` can move cache files to reference topics
- `.web-cache/` is cleared after successful reflection
- `.web-cache/` is preserved after failed reflection

## Prior Art

Old code in `../t-koma`:

- `t-koma-gateway/src/reflection.rs` — Web cache file listing for reflection prompt,
  post-reflection cache clearing logic. Directly reusable.
- `t-koma-gateway/src/tools/web_fetch.rs` — Auto-cache logic (save fetched content to
  `.web-cache/` with metadata frontmatter). Directly reusable.
