# Crawl4ai: Primary Fetch Path + Agent Control — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Make crawl4ai the primary web fetch path with agent-controllable options and live tests.

**Architecture:** HTML fetches route through crawl4ai with tuned defaults (no scrolling,
networkidle, overlay removal). The agent controls `wait_for`, `css_selector`, and
`scan_full_page` via tool params. Reqwest handles non-HTML and acts as fallback.

**Tech Stack:** Rust, crawl4ai Docker API, reqwest, serde_json, tokio (tests)

---

## Problem

1. Crawl4ai is only a fallback for HTTP errors — JS-rendered pages that return 200 with
   empty shells never trigger it
2. 40s+ per page due to `scan_full_page: true` + fixed 2s delay
3. Agent has zero control — can't request scrolling, wait for elements, or focus on a
   page region

## Design

### Flow

**Before:** reqwest GET -> extract locally -> crawl4ai only on HTTP error

**After:** HEAD request for cheap content-type routing, single fetch per page.

```
HEAD(url)
  |
  +-- 2xx with content-type
  |     |
  |     +-- HTML / xhtml           →  crawl4ai(url, options)
  |     |                               +-- success → return markdown
  |     |                               +-- failure → reqwest GET + local extraction
  |     |
  |     +-- text, JSON, XML        →  reqwest GET(url) → return raw text
  |     |
  |     +-- PDF, binary            →  error: "use reference import"
  |
  +-- HEAD failed (403, 405, timeout, etc.)
        |
        →  crawl4ai(url, options)        // handles anti-bot, JS rendering
             +-- success → return markdown
             +-- failure → error (no further fallback — if both fail, it's a real error)
```

**Key property:** One real fetch per page. HEAD is ~50ms overhead (no body). We never
download the same page twice.

**When `crawl4ai_url` is None:** Skip HEAD, do reqwest GET directly with local extraction
(same as today). This keeps `import_page` and offline use working.

### Callers

| Caller | File | Notes |
|--------|------|-------|
| `WebFetch` tool | `src/tools/web_fetch.rs:47` | Primary path — gets new params |
| `ghost web fetch` CLI | `src/cli/web.rs:75` | Passes config.crawl4ai_url, uses FetchOptions |
| `import_page` | `src/reference_import/page.rs:56` | Passes `None` for crawl4ai_url, default options — **leave unchanged** |
| Live tests | `tests/web_fetch_live.rs` | Call both `fetch` and `fetch_with_crawl4ai` directly |

`import_page` intentionally skips crawl4ai (it runs in bulk import where browser overhead
isn't worth it, and content-type routing matters). No changes needed there.

---

## Tasks

### Task 1: Add `Crawl4aiOptions` and update `browser.rs`

**Files:**
- Modify: `src/web/browser.rs`

**Step 1: Add the options struct and update `crawler_params`**

```rust
/// Options the agent can pass to control crawl4ai behavior.
#[derive(Debug, Default)]
pub struct Crawl4aiOptions {
    pub wait_for: Option<String>,
    pub css_selector: Option<String>,
    pub scan_full_page: bool,
}
```

Update `crawler_params()` to accept `&Crawl4aiOptions` and merge:

```rust
fn crawler_params(options: &Crawl4aiOptions) -> Value {
    let mut params = json!({
        "cache_mode": "bypass",
        "scan_full_page": options.scan_full_page,
        "wait_until": "networkidle",
        "page_timeout": 60000,
        "delay_before_return_html": 0.5,
        "remove_overlay_elements": true,
        "excluded_tags": ["nav", "footer", "header"],
        "word_count_threshold": 10,
        "exclude_external_links": true,
        "markdown_generator": {
            "type": "DefaultMarkdownGenerator",
            "params": {
                "content_filter": {
                    "type": "PruningContentFilter",
                    "params": {
                        "threshold": 0.4,
                        "threshold_type": "fixed",
                        "min_word_threshold": 0
                    }
                }
            }
        }
    });
    if let Some(wf) = &options.wait_for {
        params["wait_for"] = json!(wf);
    }
    if let Some(sel) = &options.css_selector {
        params["css_selector"] = json!(sel);
    }
    params
}
```

**Step 2: Update `fetch_with_crawl4ai` signature**

Change from `(base_url, page_url)` to `(base_url, page_url, options: &Crawl4aiOptions)`.
Use `crawler_params(options)` instead of `crawler_params()`.

**Step 3: Update unit tests**

Fix the two existing tests to pass `&Crawl4aiOptions::default()` and add assertions for
new defaults (`networkidle`, `remove_overlay_elements`, `scan_full_page: false`).

```rust
#[test]
fn crawler_params_defaults() {
    let params = crawler_params(&Crawl4aiOptions::default());
    assert_eq!(params["wait_until"], "networkidle");
    assert_eq!(params["scan_full_page"], false);
    assert_eq!(params["remove_overlay_elements"], true);
    assert_eq!(params["delay_before_return_html"], 0.5);
    assert!(params.get("wait_for").is_none());
    assert!(params.get("css_selector").is_none());
}

#[test]
fn crawler_params_with_options() {
    let opts = Crawl4aiOptions {
        wait_for: Some("css:.loaded".into()),
        css_selector: Some("article.main".into()),
        scan_full_page: true,
    };
    let params = crawler_params(&opts);
    assert_eq!(params["wait_for"], "css:.loaded");
    assert_eq!(params["css_selector"], "article.main");
    assert_eq!(params["scan_full_page"], true);
}
```

**Step 4: Update `mod.rs` export**

Export `Crawl4aiOptions` from `src/web/mod.rs`.

**Step 5: Run `just ci`**

This will fail on callers of `fetch_with_crawl4ai` (fetch.rs, live tests) — that's
expected and fixed in the next tasks.

**Step 6: Commit**

```
feat: add Crawl4aiOptions and tune crawl4ai defaults
```

---

### Task 2: Update `FetchOptions` and rewrite `fetch()` flow

**Files:**
- Modify: `src/web/types.rs` (FetchOptions)
- Modify: `src/web/fetch.rs` (fetch function)

**Step 1: Extend `FetchOptions` in `types.rs`**

```rust
#[derive(Debug, Clone, Default)]
pub struct FetchOptions {
    pub readability: bool,
    pub raw: bool,
    pub wait_for: Option<String>,
    pub css_selector: Option<String>,
    pub scan_full_page: bool,
}
```

**Step 2: Rewrite `fetch()` in `fetch.rs`**

Replace the current flow with HEAD-based routing. The new `fetch()` logic:

```rust
pub async fn fetch(
    url: &str,
    options: &FetchOptions,
    crawl4ai_url: Option<&str>,
) -> Result<ExtractedContent, WebError> {
    let parsed = Url::parse(url).map_err(|_| WebError::InvalidUrl { url: url.to_string() })?;
    match parsed.scheme() {
        "http" | "https" => {}
        _ => return Err(WebError::InvalidUrl { url: url.to_string() }),
    }

    let c4ai_options = super::browser::Crawl4aiOptions {
        wait_for: options.wait_for.clone(),
        css_selector: options.css_selector.clone(),
        scan_full_page: options.scan_full_page,
    };

    // When crawl4ai is available, use HEAD for cheap content-type routing.
    // When not available, fall through to the legacy GET path.
    if let Some(c4ai_url) = crawl4ai_url {
        match head_content_type(&parsed).await {
            Ok(ct) => {
                if is_html_content_type(&ct) {
                    // HTML → crawl4ai (primary), local extraction (fallback)
                    return fetch_html_via_crawl4ai(c4ai_url, url, &c4ai_options, options).await;
                } else if is_text_content(&ct) {
                    // text/JSON/XML → reqwest GET (no browser needed)
                    return fetch_text_via_reqwest(url).await;
                } else {
                    // PDF, binary, etc.
                    return Err(WebError::UnsupportedContentType { content_type: ct });
                }
            }
            Err(_) => {
                // HEAD failed (403, 405, timeout) → try crawl4ai directly
                logfire::info!(
                    "HEAD request failed, trying crawl4ai directly",
                    url = url.to_string(),
                );
                return fetch_html_via_crawl4ai(c4ai_url, url, &c4ai_options, options).await;
            }
        }
    }

    // Legacy path: no crawl4ai available — reqwest GET + local extraction
    // (used by import_page and offline mode)
    fetch_legacy(url, options).await
}
```

**Step 3: Add helper functions**

Extract the routing logic into small helpers:

```rust
/// Cheap HEAD request — returns content-type string or error.
async fn head_content_type(url: &Url) -> Result<String, WebError> {
    let response = client()
        .head(url.clone())
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(WebError::HttpStatus {
            status: response.status().as_u16(),
            url: url.to_string(),
        });
    }
    Ok(parse_content_type(response.headers()).unwrap_or_default())
}

fn is_html_content_type(ct: &str) -> bool {
    ct == "text/html" || ct == "application/xhtml+xml" || ct.is_empty()
}

/// HTML path: crawl4ai first, local extraction fallback.
async fn fetch_html_via_crawl4ai(
    c4ai_url: &str,
    page_url: &str,
    c4ai_options: &super::browser::Crawl4aiOptions,
    fetch_options: &FetchOptions,
) -> Result<ExtractedContent, WebError> {
    match super::browser::fetch_with_crawl4ai(c4ai_url, page_url, c4ai_options).await {
        Ok(markdown) => Ok(markdown_to_content(markdown, None)),
        Err(e) => {
            logfire::warn!(
                "crawl4ai failed, falling back to local extraction",
                url = page_url.to_string(),
                error = e.to_string(),
            );
            // Fallback: reqwest GET + readability/htmd
            let (html, _final_url) = fetch_raw(page_url).await?;
            Ok(extract_content(&html, page_url, fetch_options))
        }
    }
}

/// Non-HTML text path: reqwest GET, return raw text.
async fn fetch_text_via_reqwest(url: &str) -> Result<ExtractedContent, WebError> {
    let parsed = Url::parse(url).map_err(|_| WebError::InvalidUrl { url: url.to_string() })?;
    let response = client().get(parsed).send().await?;
    if !response.status().is_success() {
        return Err(WebError::HttpStatus {
            status: response.status().as_u16(),
            url: url.to_string(),
        });
    }
    let bytes = response.bytes().await?;
    let text = String::from_utf8_lossy(&bytes).replace('\0', "");
    let (text, truncated) = truncate(text, MAX_EXTRACT_CHARS);
    let word_count = text.split_whitespace().count();
    Ok(ExtractedContent { title: None, text, word_count, truncated })
}

/// Legacy path: reqwest GET + local extraction (no crawl4ai).
/// Used when crawl4ai_url is None (import_page, offline).
async fn fetch_legacy(url: &str, options: &FetchOptions) -> Result<ExtractedContent, WebError> {
    // This is essentially the old fetch() body — reqwest GET, content-type check,
    // HTML → extract_content, non-HTML text → raw, binary → error.
    // Move the existing logic here unchanged.
}
```

The old `fetch()` body moves into `fetch_legacy()` with minimal changes (just remove
the crawl4ai fallback branches since those are now handled by the new flow).

**Step 4: Run `just ci`**

Should compile. Existing unit tests in `fetch.rs` test `extract_content()` directly
(not `fetch()`), so they should pass unchanged.

**Step 5: Commit**

```
feat: HEAD-based routing with crawl4ai as primary HTML path
```

---

### Task 3: Update `web_fetch` tool schema

**Files:**
- Modify: `src/tools/web_fetch.rs`

**Step 1: Add new properties to input schema**

```rust
fn schema(&self) -> ToolDefinition {
    ToolDefinition {
        name: self.name().to_string(),
        description: "Fetch and extract the text content of a web page. Uses a headless \
                      browser for JavaScript-rendered pages. Content is automatically \
                      cached for later reference curation.\n\n\
                      Options:\n\
                      - wait_for: wait for a CSS selector (css:.content) or JS condition \
                        (js:() => document.querySelectorAll('.item').length > 10) before \
                        extracting. Use when content loads dynamically.\n\
                      - css_selector: restrict extraction to a specific DOM region \
                        (e.g. 'article', 'main', '#content'). Reduces noise.\n\
                      - scan_full_page: scroll the entire page to trigger lazy-loaded \
                        content. Slower — only use for infinite-scroll or long list pages."
            .to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "The URL to fetch."
                },
                "wait_for": {
                    "type": "string",
                    "description": "CSS selector (css:<sel>) or JS condition (js:<code>) to wait for before extraction."
                },
                "css_selector": {
                    "type": "string",
                    "description": "CSS selector to focus extraction on (e.g. 'article', '#main-content')."
                },
                "scan_full_page": {
                    "type": "boolean",
                    "description": "Scroll full page for lazy-loaded content. Default: false."
                }
            },
            "required": ["url"]
        }),
    }
}
```

**Step 2: Parse new params in `execute()`**

```rust
let options = FetchOptions {
    wait_for: params.get("wait_for").and_then(Value::as_str).map(String::from),
    css_selector: params.get("css_selector").and_then(Value::as_str).map(String::from),
    scan_full_page: params.get("scan_full_page").and_then(Value::as_bool).unwrap_or(false),
    ..Default::default()
};
```

**Step 3: Run `just ci`**

**Step 4: Commit**

```
feat: expose crawl4ai options in web_fetch tool schema
```

---

### Task 4: Update CLI caller

**Files:**
- Modify: `src/cli/web.rs`

**Step 1: Add CLI args for new options**

Add to `WebCommand::Fetch`:

```rust
Fetch {
    url: String,
    #[arg(long, conflicts_with = "raw")]
    readability: bool,
    #[arg(long, conflicts_with = "readability")]
    raw: bool,
    /// CSS selector or JS condition to wait for (e.g. "css:.content-loaded")
    #[arg(long)]
    wait_for: Option<String>,
    /// Focus extraction on a CSS selector region
    #[arg(long)]
    css_selector: Option<String>,
    /// Scroll full page for lazy-loaded content
    #[arg(long)]
    scan_full_page: bool,
},
```

**Step 2: Map to FetchOptions**

```rust
let options = web::FetchOptions {
    readability,
    raw,
    wait_for,
    css_selector,
    scan_full_page,
};
```

**Step 3: Run `just ci`**

**Step 4: Commit**

```
feat: add crawl4ai options to ghost web fetch CLI
```

---

### Task 5: Update existing live tests

**Files:**
- Modify: `tests/web_fetch_live.rs`

**Step 1: Fix all `fetch_with_crawl4ai` calls**

Update every call to pass `&Crawl4aiOptions::default()` as the third argument.
Add import: `use ghost::web::Crawl4aiOptions;`

**Step 2: Run live tests**

```bash
CRAWL4AI_URL=http://localhost:11235 cargo test --features live-tests web_fetch -- --nocapture
```

Verify existing tests still pass with the new defaults (networkidle, no full scroll).
Some word counts may change — adjust assertions if needed, but don't weaken them.

**Step 3: Commit**

```
test: update existing live tests for new crawl4ai signature
```

---

### Task 6: New live tests — crawl4ai primary path

**Files:**
- Modify: `tests/web_fetch_live.rs`

Add these tests after the existing ones:

**Test 1: `crawl4ai_wikipedia_fast`** — Speed baseline

```rust
/// Wikipedia should be fast with the new defaults (no full-page scroll).
#[tokio::test]
async fn crawl4ai_wikipedia_fast() {
    let url = "https://en.wikipedia.org/wiki/3D_printing";
    eprintln!("\n=== Wikipedia Speed Baseline ===");

    let start = std::time::Instant::now();
    let result = fetch(
        url,
        &FetchOptions::default(),
        Some(&crawl4ai_url()),
    )
    .await
    .expect("fetch should succeed");

    let elapsed = start.elapsed();
    save("wikipedia", url, "crawl4ai_primary", result.word_count, &result.text);
    eprintln!("  {} words in {:.1}s", result.word_count, elapsed.as_secs_f64());

    assert!(result.word_count > 1000, "Wikipedia article should have substance");
    assert!(
        elapsed.as_secs() < 30,
        "Wikipedia fetch should complete in < 30s (took {:.1}s)",
        elapsed.as_secs_f64()
    );
}
```

**Test 2: `crawl4ai_all3dp_full_list`** — scan_full_page for deep content

```rust
/// All3DP big list — needs scan_full_page to reach items near the bottom.
#[tokio::test]
async fn crawl4ai_all3dp_full_list() {
    let url = "https://all3dp.com/1/best-3d-printer-reviews-top-3d-printers-home-3-d-printer-3d/";
    eprintln!("\n=== All3DP Full List (scan_full_page) ===");

    let result = fetch(
        url,
        &FetchOptions {
            scan_full_page: true,
            ..Default::default()
        },
        Some(&crawl4ai_url()),
    )
    .await
    .expect("fetch should succeed");

    save("all3dp_full_list", url, "crawl4ai_scroll", result.word_count, &result.text);
    eprintln!("  {} words", result.word_count);

    let text_lower = result.text.to_lowercase();
    assert!(
        text_lower.contains("anycubic photon mono m7 max")
            || text_lower.contains("photon mono m7"),
        "Should find Anycubic Photon Mono M7 Max deep in the page"
    );
}
```

**Test 3: `crawl4ai_github_issue`** — JS-rendered page

```rust
/// GitHub issue — JS-rendered, reqwest alone gets a shell.
#[tokio::test]
async fn crawl4ai_github_issue() {
    // A well-known, stable open issue
    let url = "https://github.com/rust-lang/rust/issues/34511";
    eprintln!("\n=== GitHub Issue (JS-rendered) ===");

    let result = fetch(
        url,
        &FetchOptions::default(),
        Some(&crawl4ai_url()),
    )
    .await
    .expect("fetch should succeed");

    save("github_issue", url, "crawl4ai_primary", result.word_count, &result.text);
    eprintln!("  {} words", result.word_count);

    assert!(
        result.text.contains("Are we async yet?"),
        "Should contain the issue title"
    );
}
```

**Test 4: `crawl4ai_css_selector`** — focused extraction

```rust
/// CSS selector should focus extraction and produce less noise.
#[tokio::test]
async fn crawl4ai_css_selector() {
    let url = "https://en.wikipedia.org/wiki/3D_printing";
    eprintln!("\n=== CSS Selector Focus ===");

    // Full page
    let full = fetch_with_crawl4ai(
        &crawl4ai_url(),
        url,
        &Crawl4aiOptions::default(),
    )
    .await
    .expect("full fetch");
    let full_words = full.split_whitespace().count();

    // Focused on just the article body
    let focused = fetch_with_crawl4ai(
        &crawl4ai_url(),
        url,
        &Crawl4aiOptions {
            css_selector: Some("#mw-content-text".into()),
            ..Default::default()
        },
    )
    .await
    .expect("focused fetch");
    let focused_words = focused.split_whitespace().count();

    save("wikipedia_full", url, "crawl4ai_full", full_words, &full);
    save("wikipedia_focused", url, "crawl4ai_selector", focused_words, &focused);
    eprintln!("  full: {full_words} words, focused: {focused_words} words");

    // Focused should have content but be more concise (less nav/sidebar)
    assert!(focused_words > 500, "focused should still have substance");
}
```

**Test 5: `crawl4ai_fallback_to_local`** — crawl4ai down, local extraction works

```rust
/// When crawl4ai is unavailable, fetch() falls back to local extraction.
#[tokio::test]
async fn crawl4ai_fallback_to_local() {
    let url = "https://en.wikipedia.org/wiki/3D_printing";
    let bad_url = "http://localhost:1"; // nothing listening
    eprintln!("\n=== Fallback to Local Extraction ===");

    let result = fetch(
        url,
        &FetchOptions::default(),
        Some(bad_url),
    )
    .await
    .expect("should fall back to local extraction");

    save("wikipedia_fallback", url, "local_fallback", result.word_count, &result.text);
    eprintln!("  {} words (local extraction)", result.word_count);

    assert!(result.word_count > 500, "local fallback should produce content");
}
```

**Step 2: Run all live tests**

```bash
CRAWL4AI_URL=http://localhost:11235 cargo test --features live-tests web_fetch -- --nocapture
```

**Step 3: Commit**

```
test: add crawl4ai live tests for primary path, scroll, selector, fallback
```

---

### Task 7: Final CI + cleanup

**Step 1: Run `just ci`**

Fix any clippy warnings or formatting issues.

**Step 2: Verify the spec file is up to date**

Update `specs/w_crawl4ai.md` to reflect the completed work (mark issues resolved,
remove stale TODOs).

**Step 3: Commit**

```
chore: ci fixes and spec update
```
