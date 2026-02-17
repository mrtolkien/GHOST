---
name: web-search
description:
  Advanced web search and fetch techniques. Covers search strategies, fetch
  modes, result curation, and effective research workflows.
triggers:
  - search the web
  - look up online
  - find on the internet
---

# Web Search Skill

Advanced usage of `ghost web search` and `ghost web fetch` for effective research.

## Search Basics

```
ghost web search "<query>" [-n <max_results>]
```

- Default: 5 results. Use `-n 10` for broader coverage.
- Results include title, URL, and snippet.
- All results are auto-cached to `$WORKSPACE/.web-cache/`.

### Writing Effective Queries

- **Be specific**: `"rust surrealdb embedded driver example"` not `"database"`
- **Use quotes for exact phrases**: `"connection pooling" rust tokio`
- **Include version numbers**: `"react 19 server components"` not just
  `"react
  server components"`
- **Add context keywords**: `"serde custom deserializer derive macro"` not
  `"serde deserialize"`

## Fetch Modes

```
ghost web fetch "<url>" [--max-chars <N>] [--readability] [--raw]
```

### Choosing the Right Mode

- **Default** (no flags): Full HTML to Markdown. All page content preserved. Best for:
  - Documentation pages, API references
  - Index/listing pages, homepages
  - Search result pages, forums
  - Any page where you need the complete content

- **`--readability`**: Extracts only the main article body, stripping nav, sidebars,
  headers, footers. Best for:
  - Blog posts and news articles
  - Essays, tutorials, long-form writing
  - Any page with a single primary article

- **`--raw`**: Returns raw HTML with no conversion. Best for:
  - Debugging Markdown conversion issues
  - Pages with important structural info lost in conversion
  - Inspecting page source

### Truncation

Use `--max-chars <N>` to limit output size (default 50000). Useful for:

- Large documentation pages: `--max-chars 20000`
- Quick overview: `--max-chars 5000`

## Research Workflows

### Quick Fact Check

1. `ghost web search "specific question"`
2. Read snippets — often sufficient for facts
3. Fetch one authoritative source if needed

### Deep Research

1. `ghost web search "topic overview"` — identify key sources
2. `ghost web fetch` the top 2-3 results
3. `ghost web search "topic specific aspect"` — fill gaps
4. `ghost web fetch` targeted pages
5. Synthesize into a knowledge note

### Documentation Lookup

1. `ghost web search "library_name docs <feature>"`
2. `ghost web fetch` the docs page with `--readability` for clean reading
3. If the page has navigation/sidebar links, re-fetch without `--readability` to see the
   full structure

## Caching

All fetched content is cached to `$WORKSPACE/.web-cache/` with metadata. Cache paths are
printed to stderr on fetch.

- Cache prevents redundant fetches within a session
- Cached pages can be read later with `read_file`
- Cache files include a YAML frontmatter with the source URL and fetch date
