# Backlog — Reference Import Tool

## Overview

Bulk import documentation sites, code repositories, or web page collections into
searchable reference topics.

## Source Types

### Git

Clone a repository and import relevant files:

```json
{
  "source": "git",
  "url": "https://github.com/surrealdb/surrealdb",
  "paths": ["doc/", "README.md"],
  "extensions": [".md", ".rs"]
}
```

### Web (Single Page)

Import a single web page as a reference file.

### Crawl (BFS)

Crawl from a seed URL following same-host links:

```json
{
  "source": "crawl",
  "url": "https://docs.surrealdb.com",
  "max_depth": 3,
  "max_pages": 50
}
```

## Why Deferred

- Complex to implement well (crawl rate limiting, content extraction quality)
- `web_fetch` covers single-page imports for PoC
- Bulk import can be done manually with shell commands for now
