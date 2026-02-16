# 11 — Web Search and Web Fetch

## Overview

Web tools allow the GHOST to search the internet and fetch web page contents. Web
results are automatically cached for later curation during reflection.

## Web Search

### Provider: Brave Search API

```rust
pub struct BraveSearchProvider {
    api_key: String,
    max_results: usize,
    client: reqwest::Client,
}
```

- **API**: `https://api.search.brave.com/res/v1/web/search`
- **Auth**: `X-Subscription-Token` header
- **Config**: `web.search_provider = "brave"`, `web.search_max_results = 5`

### Tool: `web_search`

- Parameters: `query: string`
- Returns: List of results with title, URL, and snippet
- Results are NOT auto-cached (they're just search result snippets)

## Web Fetch

### Tool: `web_fetch`

- Parameters: `url: string`, `max_chars: number (optional, default 50000)`
- Returns: Extracted text content (HTML → readable text)
- Uses Mozilla Readability algorithm (via `readability` crate) for article extraction
- Falls back to `html2text` for non-article pages

### Auto-Caching

Successful web fetches are automatically saved to `$WORKSPACE/.web-cache/` for later
curation during reflection:

```
.web-cache/
├── 2025-02-15T14-30-00_example-com_page-title.md
├── 2025-02-15T14-35-00_docs-rs_surrealdb-api.md
└── ...
```

File format:

```markdown
---
url: https://example.com/page
fetched_at: 2025-02-15T14:30:00Z
---

# Page Title

[extracted content...]
```

Non-2xx responses are NOT cached (the GHOST sees the error but nothing is saved).

## Content Extraction

```rust
pub fn extract_content(html: &str, url: &str) -> ExtractedContent {
    // Try readability first (works well for articles)
    // Fall back to html2text (works for everything)
}

pub struct ExtractedContent {
    pub title: Option<String>,
    pub text: String,
    pub word_count: usize,
}
```

## Observability

```rust
#[tracing::instrument(skip_all, fields(url = %url))]
async fn fetch(&self, url: &str) -> Result<ExtractedContent> {
    let response = self.client.get(url).send().await?;
    logfire::info!("web fetch",
        url = %url,
        status = response.status().as_u16(),
        content_length = response.content_length(),
    );
    // ...
}
```

## Validation

1. `cargo test --features live-tests` — `web_search` with a known query returns Brave
   results with titles, URLs, and snippets (requires `BRAVE_API_KEY`)
2. `cargo test --features live-tests` — `web_fetch` on a known URL returns extracted
   text content
3. `cargo test` — after a successful mock fetch, a cache file exists in
   `$WORKSPACE/.web-cache/` with correct frontmatter (url, fetched_at)
4. `cargo test` — a failed fetch (mock 404) does NOT create a cache file
5. `cargo test` — `max_chars` truncation: fetch a large mock page, verify output is
   capped
6. `just ci` — passes

## Acceptance Criteria

- `web_search` returns Brave Search results with titles, URLs, and snippets
- `web_fetch` extracts readable text from HTML pages
- Successful fetches are auto-cached to `.web-cache/`
- Cache files include URL and timestamp metadata
- Non-2xx responses are not cached
- Large pages are truncated at `max_chars`
- All web operations produce tracing spans
- API keys are loaded from environment variables
- `just ci` passes

## Prior Art

Old code in `../t-koma`:

- `t-koma-gateway/src/tools/web_search.rs` — Brave Search API integration. Directly
  reusable.
- `t-koma-gateway/src/tools/web_fetch.rs` — Web fetch with readability extraction and
  auto-caching. Directly reusable.
- `t-koma-gateway/src/web/` — Web search provider abstraction, content extraction
  utilities (`readability`, `html2text`). Reusable.
